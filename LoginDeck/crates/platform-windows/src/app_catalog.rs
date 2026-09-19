//! Bounded discovery/import for Windows. Verification and launch follow in Task 7.

pub(crate) mod package;
pub(crate) mod shortcut;
pub(crate) mod win32;

use crate::WindowsLaunchTarget;
use autologin_core::{
    AppError, ApplicationCatalog, ApplicationRecord, DiscoveredApplication, DiscoverySource,
    Platform, VerifiedApplication,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub(crate) const MAX_SOURCE_ITEMS: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct IconSource {
    pub(crate) path: PathBuf,
    pub(crate) index: i32,
}

/// Discovery metadata only. The fingerprint is not publisher/Authenticode trust.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowsAppCandidate {
    pub(crate) identity: String,
    pub(crate) display_name: String,
    pub(crate) version: Option<String>,
    pub(crate) target: WindowsLaunchTarget,
    pub(crate) icon_sources: Vec<IconSource>,
    pub(crate) signature_identity: String,
    /// Only an OS-associated, independently checked executable fingerprint can
    /// prove a packaged-desktop overlap. A path or a display name is insufficient.
    pub(crate) executable_identity: Option<String>,
    pub(crate) sources: Vec<String>,
    pub(crate) alternate_identities: Vec<String>,
}

#[derive(Default)]
pub struct WindowsApplicationCatalog;

impl WindowsApplicationCatalog {
    pub fn new() -> Self {
        Self
    }
}

type SourceResult = Result<Vec<WindowsAppCandidate>, AppError>;

fn discovery_error() -> AppError {
    AppError::new("application.discovery_unavailable")
}
fn import_error() -> AppError {
    AppError::new("application.unsupported_import")
}

fn valid_candidate(candidate: &WindowsAppCandidate) -> bool {
    // Validate without invoking encode on a malformed directly constructed variant.
    let target = match &candidate.target {
        WindowsLaunchTarget::Executable(path) => {
            let Some(value) = path.to_str() else {
                return false;
            };
            if !win32::local_path(value) {
                return false;
            }
            format!("exe:{value}")
        }
        WindowsLaunchTarget::Aumid(aumid) => {
            if aumid.len() > 512 || candidate.identity != *aumid || !package::valid_aumid(aumid) {
                return false;
            }
            format!("aumid:{aumid}")
        }
    };
    !candidate.signature_identity.is_empty()
        && !candidate.display_name.chars().any(char::is_control)
        && !candidate.identity.chars().any(char::is_control)
        && !candidate.signature_identity.chars().any(char::is_control)
        && WindowsLaunchTarget::parse(&target).is_ok()
        && autologin_core::validate_application_metadata(
            &candidate.display_name,
            &candidate.identity,
            &target,
            &[],
            candidate.version.as_deref(),
            &candidate.signature_identity,
            None,
        )
        .is_ok()
}

pub(crate) fn merge_candidates(
    win32: Vec<WindowsAppCandidate>,
    packaged: Vec<WindowsAppCandidate>,
) -> Vec<WindowsAppCandidate> {
    let win32 = win32::deduplicate(
        win32
            .into_iter()
            .take(MAX_SOURCE_ITEMS)
            .filter(valid_candidate)
            .filter(|item| matches!(item.target, WindowsLaunchTarget::Executable(_)))
            .collect(),
    );
    let mut packaged: Vec<_> = packaged
        .into_iter()
        .take(MAX_SOURCE_ITEMS)
        .filter(valid_candidate)
        .filter(|item| matches!(item.target, WindowsLaunchTarget::Aumid(_)))
        .collect();
    packaged.sort_by_key(|item| {
        (
            item.identity.clone(),
            item.signature_identity.clone(),
            item.display_name.clone(),
            item.executable_identity.clone(),
        )
    });
    let mut unique = BTreeMap::new();
    for item in packaged {
        unique
            .entry(item.identity.to_ascii_lowercase())
            .or_insert(item);
    }
    let proven: BTreeSet<_> = unique
        .values()
        .filter_map(|item| item.executable_identity.as_deref())
        .filter(|identity| identity.starts_with("winfile-v1:") && identity.len() <= 16 * 1024)
        .collect();
    let mut result: Vec<_> = win32
        .into_iter()
        .filter(|item| !proven.contains(item.signature_identity.as_str()))
        .collect();
    result.extend(unique.into_values());
    result.sort_by_key(|item| (item.display_name.to_lowercase(), item.identity.clone()));
    result.truncate(MAX_SOURCE_ITEMS);
    result
}

fn discovered(candidate: WindowsAppCandidate, source: DiscoverySource) -> DiscoveredApplication {
    DiscoveredApplication {
        platform: Platform::Windows,
        display_name: candidate.display_name,
        platform_application_id: candidate.identity,
        launch_target: candidate.target.encode(),
        alternate_launch_targets: vec![],
        path_access_ref: None,
        version: candidate.version,
        signature_identity: candidate.signature_identity,
        discovery_source: source,
    }
}

fn combine_sources(
    win32: SourceResult,
    packaged: SourceResult,
) -> Result<Vec<DiscoveredApplication>, AppError> {
    if win32.is_err() && packaged.is_err() {
        return Err(discovery_error());
    }
    Ok(
        merge_candidates(win32.unwrap_or_default(), packaged.unwrap_or_default())
            .into_iter()
            .map(|item| discovered(item, DiscoverySource::Automatic))
            .collect(),
    )
}

async fn discover_with<F>(enumerate: F) -> Result<Vec<DiscoveredApplication>, AppError>
where
    F: FnOnce() -> (SourceResult, SourceResult) + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let (win32, packaged) = enumerate();
        combine_sources(win32, packaged)
    })
    .await
    .map_err(|_| discovery_error())?
}

#[async_trait::async_trait]
impl ApplicationCatalog for WindowsApplicationCatalog {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        discover_with(|| {
            (
                win32::Win32Inventory.enumerate(),
                package::PackagedAppInventory.enumerate_current_user(),
            )
        })
        .await
    }

    async fn import_bundle(&self, path: &Path) -> Result<Vec<DiscoveredApplication>, AppError> {
        // Reject oversized/invalid input before cloning across the blocking boundary.
        if !path.to_str().is_some_and(win32::local_path) {
            return Err(import_error());
        }
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || import_path(&path).map(|item| vec![item]))
            .await
            .map_err(|_| import_error())?
    }

    async fn verify_and_launch(
        &self,
        _app: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError> {
        // Intentionally cannot launch until Task 7 implements atomic revalidation.
        Err(AppError::new("application.not_supported"))
    }
}

fn protected_package_path(path: &Path) -> bool {
    path.to_str().is_none_or(|value| {
        value
            .split(['\\', '/'])
            .any(|part| part.eq_ignore_ascii_case("WindowsApps"))
    })
}

#[cfg(windows)]
fn import_path(path: &Path) -> Result<DiscoveredApplication, AppError> {
    import_path_with(path, || win32::Win32Inventory.enumerate())
}

#[cfg(windows)]
fn import_path_with(
    path: &Path,
    enumerate_win32: impl FnOnce() -> SourceResult,
) -> Result<DiscoveredApplication, AppError> {
    use std::{fs, os::windows::fs::MetadataExt};
    use win32::filesystem::{checked_directory, executable};
    const MAX_DEPTH: usize = 2;
    const MAX_ENTRIES: usize = 256;
    let value = path
        .to_str()
        .filter(|value| win32::local_path(value))
        .ok_or_else(import_error)?;
    if protected_package_path(path) {
        return Err(import_error());
    }

    fn eligible(path: &Path) -> Option<win32::SafeExecutable> {
        let value = path.to_str()?;
        if protected_package_path(path) || win32::is_helper(value) {
            return None;
        }
        let exe = executable(value)?;
        (!protected_package_path(&exe.path) && !win32::is_helper(exe.path.to_str()?)).then_some(exe)
    }

    let selected = if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        eligible(path).ok_or_else(import_error)?
    } else {
        let root = checked_directory(value).ok_or_else(import_error)?;
        if protected_package_path(&root.path) || !win32::narrow_install_root(&root.path) {
            return Err(import_error());
        }
        let mut pending = vec![(root.path.clone(), 0)];
        let mut visited = 0;
        let mut selected = None;
        while let Some((directory, depth)) = pending.pop() {
            let checked = checked_directory(directory.to_str().ok_or_else(import_error)?)
                .ok_or_else(import_error)?;
            if protected_package_path(&checked.path)
                || (checked.path != root.path && !win32::within(&checked.path, &root.path))
            {
                return Err(import_error());
            }
            // Keep both root and current-directory guards while enumerating.
            for child in fs::read_dir(&checked.path).map_err(|_| import_error())? {
                visited += 1;
                if visited > MAX_ENTRIES {
                    return Err(import_error());
                }
                let child = child.map_err(|_| import_error())?;
                let child_path = child.path();
                if protected_package_path(&child_path) {
                    return Err(import_error());
                }
                let metadata = fs::symlink_metadata(&child_path).map_err(|_| import_error())?;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(import_error());
                }
                if metadata.is_dir() {
                    if depth == MAX_DEPTH {
                        return Err(import_error());
                    }
                    let child = checked_directory(child_path.to_str().ok_or_else(import_error)?)
                        .ok_or_else(import_error)?;
                    if !win32::within(&child.path, &root.path)
                        || protected_package_path(&child.path)
                    {
                        return Err(import_error());
                    }
                    pending.push((child.path.clone(), depth + 1));
                } else if metadata.is_file()
                    && child_path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                    && !win32::is_helper(child_path.to_str().ok_or_else(import_error)?)
                {
                    let exe = eligible(&child_path).ok_or_else(import_error)?;
                    if !win32::within(&exe.path, &root.path) || selected.is_some() {
                        return Err(import_error());
                    }
                    selected = Some(exe);
                }
            }
        }
        selected.ok_or_else(import_error)?
    };
    // Use the same bounded validation and deterministic Win32 deduplication as
    // automatic discovery. A failed lookup cannot establish that this file is
    // unregistered, so do not persist a conflicting manual identity on failure.
    let inventory = enumerate_win32().map_err(|_| discovery_error())?;
    let selected_path = win32::path_key(&selected.path);
    if let Some(registered) = merge_candidates(inventory, vec![])
        .into_iter()
        .find(|item| {
            matches!(&item.target, WindowsLaunchTarget::Executable(path)
            if win32::path_key(path) == selected_path)
        })
    {
        // Reject a changed file observed during lookup instead of associating it
        // with stale metadata or silently falling back to a new manual identity.
        if registered.signature_identity != selected.fingerprint {
            return Err(import_error());
        }
        return Ok(discovered(registered, DiscoverySource::ManualImport));
    }
    let display_name = selected
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(import_error)?
        .to_owned();
    let candidate = WindowsAppCandidate {
        identity: win32::identity_key("manual", &selected.path),
        display_name,
        version: None,
        target: WindowsLaunchTarget::Executable(selected.path),
        icon_sources: vec![],
        signature_identity: selected.fingerprint,
        executable_identity: None,
        sources: vec!["manual".into()],
        alternate_identities: vec![],
    };
    if !valid_candidate(&candidate) {
        return Err(import_error());
    }
    Ok(discovered(candidate, DiscoverySource::ManualImport))
}

#[cfg(not(windows))]
fn import_path(_path: &Path) -> Result<DiscoveredApplication, AppError> {
    Err(import_error())
}

#[cfg(test)]
mod tests {
    use super::*;
    use autologin_core::{AppError, DiscoverySource, Platform};

    fn packaged(id: &str, name: &str, proof: Option<&str>) -> WindowsAppCandidate {
        WindowsAppCandidate {
            identity: id.into(),
            display_name: name.into(),
            version: None,
            target: WindowsLaunchTarget::Aumid(id.into()),
            icon_sources: vec![],
            signature_identity: "package-v1:registration:application".into(),
            executable_identity: proof.map(str::to_owned),
            sources: vec!["package".into()],
            alternate_identities: vec![],
        }
    }
    fn win32(id: &str, name: &str, path: &str, proof: &str) -> WindowsAppCandidate {
        let mut candidate = packaged(id, name, None);
        candidate.target = WindowsLaunchTarget::Executable(path.into());
        candidate.signature_identity = proof.into();
        candidate
    }

    #[test]
    fn merge_prefers_aumid_only_when_verified_executable_identity_matches() {
        let win = win32(
            "win32:chat",
            "Desktop",
            r"C:\Apps\chat.exe",
            "winfile-v1:same-file",
        );
        let pkg = packaged("Contoso.Chat_abc!App", "Chat", Some("winfile-v1:same-file"));
        let result = merge_candidates(vec![win.clone()], vec![pkg.clone()]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].target.encode(), "aumid:Contoso.Chat_abc!App");
        for proof in [
            None,
            Some("winfile-v1:different-file"),
            Some("same-name-or-path"),
        ] {
            let mut other = pkg.clone();
            other.display_name = win.display_name.clone();
            other.executable_identity = proof.map(str::to_owned);
            assert_eq!(merge_candidates(vec![win.clone()], vec![other]).len(), 2);
        }
    }

    #[test]
    fn merge_deduplicates_aumid_preserves_equal_names_and_orders_stably() {
        let a = packaged("Contoso.Chat_abc!App", "chat", None);
        let b = packaged("Contoso.Tools_abc!Editor", "Chat", None);
        let c = packaged("Contoso.Alpha_abc!App", "Alpha", None);
        let w = win32(
            "win32:other",
            "Chat",
            r"C:\Other\chat.exe",
            "winfile-v1:other-file",
        );
        let mut candidates = vec![b.clone(), a.clone(), a.clone(), c.clone()];
        let expected = merge_candidates(vec![w.clone(), w.clone()], candidates.clone());
        candidates.reverse();
        assert_eq!(expected, merge_candidates(vec![w], candidates));
        assert_eq!(
            expected
                .iter()
                .map(|x| x.identity.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Contoso.Alpha_abc!App",
                "Contoso.Chat_abc!App",
                "Contoso.Tools_abc!Editor",
                "win32:other"
            ]
        );
        let mut same_id = a.clone();
        same_id.identity = same_id.identity.to_ascii_lowercase();
        same_id.target = WindowsLaunchTarget::Aumid(same_id.identity.clone());
        assert_eq!(merge_candidates(vec![], vec![same_id, a]).len(), 1);
    }

    #[test]
    fn discovery_failure_semantics_preserve_successful_empty_sources_and_sanitize() {
        let fail = || Err(AppError::new("private-system-detail"));
        assert!(combine_sources(fail(), Ok(vec![])).unwrap().is_empty());
        assert!(combine_sources(Ok(vec![]), fail()).unwrap().is_empty());
        let error = combine_sources(fail(), fail()).unwrap_err();
        assert_eq!(error.code(), "application.discovery_unavailable");
        assert!(error.params.is_empty());
        let item = packaged("Contoso.Chat_abc!App", "Chat", None);
        let result = combine_sources(fail(), Ok(vec![item])).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].platform, Platform::Windows);
        assert_eq!(result[0].discovery_source, DiscoverySource::Automatic);
        assert_eq!(result[0].path_access_ref, None);
        assert!(result[0].alternate_launch_targets.is_empty());
    }

    #[tokio::test]
    async fn blocking_worker_failure_is_sanitized() {
        let error = discover_with(|| panic!("private-system-detail"))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "application.discovery_unavailable");
        assert!(error.params.is_empty());
    }

    #[test]
    fn merge_bounds_raw_sources_and_final_results_and_skips_malformed_records() {
        let mut invalid = packaged("Contoso.Invalid_abc!App", "", None);
        invalid.target = WindowsLaunchTarget::Aumid("".into());
        let valid = packaged("Contoso.Chat_abc!App", "Chat", None);
        let mut records = vec![invalid; 4096];
        records.push(valid.clone());
        assert!(merge_candidates(vec![], records).is_empty());
        let mut records: Vec<_> = (0..4096)
            .map(|i| packaged(&format!("Contoso.App{i}_abc!App"), "Chat", None))
            .collect();
        records.push(valid);
        let win: Vec<_> = (0..4096)
            .map(|i| {
                win32(
                    &format!("win32:{i}"),
                    "Chat",
                    &format!(r"C:\Apps\app{i}.exe"),
                    &format!("winfile-v1:{i}"),
                )
            })
            .collect();
        assert_eq!(merge_candidates(win, records).len(), 4096);
    }

    #[cfg(windows)]
    mod import {
        use super::*;
        use std::{
            fs,
            path::{Path, PathBuf},
            process::Command,
        };
        struct Temp(PathBuf);
        impl Temp {
            fn new() -> Self {
                let path = std::env::temp_dir()
                    .join(format!("logindeck-import-test-{}", uuid::Uuid::new_v4()));
                fs::create_dir(&path).unwrap();
                Self(path)
            }
            fn file(&self, name: &str) -> PathBuf {
                let path = self.0.join(name);
                fs::write(&path, b"inert fixture; never launched").unwrap();
                path
            }
        }
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn registration(
            path: &Path,
            scope: win32::RegistryScope,
            key: &str,
            name: &str,
        ) -> WindowsAppCandidate {
            let text = |value: String| {
                Some(win32::RegistryText {
                    value,
                    expandable: false,
                })
            };
            win32::normalize_uninstall_entry(
                win32::UninstallEntry {
                    scope,
                    key: key.into(),
                    display_name: text(name.into()),
                    display_version: None,
                    display_icon: text(path.to_str().unwrap().into()),
                    install_location: None,
                    system_component: 0,
                    release_type: None,
                    parent_key: None,
                },
                &[],
                &win32::NativePathProbe,
            )
            .unwrap()
        }

        #[test]
        fn import_reuses_automatic_identity_independent_of_name_scope_order_and_path_case() {
            let tree = Temp::new();
            let path = tree.file("Chat.exe");
            let user = registration(
                &path,
                win32::RegistryScope::User64,
                "FixtureUser",
                "Registered Product",
            );
            let machine = registration(
                &path,
                win32::RegistryScope::Machine32,
                "FixtureMachine",
                "Other registered name",
            );
            let records = vec![machine, user.clone()];
            let automatic = combine_sources(Ok(records.clone()), Ok(vec![]))
                .unwrap()
                .remove(0);
            assert_eq!(automatic.platform_application_id, user.identity);
            let case_alias = PathBuf::from(path.to_str().unwrap().to_uppercase());
            let imported = import_path_with(&case_alias, || Ok(records.clone())).unwrap();
            assert_eq!(
                imported.platform_application_id,
                automatic.platform_application_id
            );
            assert_eq!(imported.launch_target, automatic.launch_target);
            assert_eq!(imported.signature_identity, automatic.signature_identity);
            assert_eq!(imported.display_name, "Registered Product");
            assert_eq!(imported.discovery_source, DiscoverySource::ManualImport);
            let mut reversed = records;
            reversed.reverse();
            assert_eq!(
                imported,
                import_path_with(&tree.0, || Ok(reversed)).unwrap()
            );
        }

        #[test]
        fn unregistered_import_fallback_is_stable_and_does_not_merge_by_name_or_fingerprint_only() {
            let tree = Temp::new();
            let path = tree.file("Chat.exe");
            let other = Temp::new();
            let mut unrelated = registration(
                &other.file("Chat.exe"),
                win32::RegistryScope::User64,
                "OtherFixture",
                "Chat",
            );
            let imported = import_path_with(&path, || Ok(vec![])).unwrap();
            // Even a claimed equal fingerprint cannot bridge a different canonical path.
            unrelated.signature_identity = imported.signature_identity.clone();
            let second = import_path_with(&tree.0, || Ok(vec![unrelated.clone()])).unwrap();
            assert_eq!(imported, second);
            assert_ne!(imported.platform_application_id, unrelated.identity);
            assert_eq!(imported.discovery_source, DiscoverySource::ManualImport);
            let case_alias = PathBuf::from(path.to_str().unwrap().to_uppercase());
            assert_eq!(
                imported,
                import_path_with(&case_alias, || Ok(vec![])).unwrap()
            );
        }

        #[test]
        fn import_rejects_same_path_with_changed_fingerprint_instead_of_creating_manual_identity() {
            let tree = Temp::new();
            let path = tree.file("Chat.exe");
            let registered = registration(&path, win32::RegistryScope::User64, "Fixture", "Chat");
            fs::write(&path, b"changed inert fixture with a different size").unwrap();
            let error = import_path_with(&path, || Ok(vec![registered])).unwrap_err();
            assert_eq!(error.code(), "application.unsupported_import");
            assert!(error.params.is_empty());
        }

        #[test]
        fn import_inventory_failure_is_sanitized_and_never_falls_back_to_manual_identity() {
            let tree = Temp::new();
            let error = import_path_with(&tree.file("Chat.exe"), || {
                Err(AppError::new("private-registry-detail"))
            })
            .unwrap_err();
            assert_eq!(error.code(), "application.discovery_unavailable");
            assert!(error.params.is_empty());
            assert_eq!(
                import_path_with(Path::new("relative.exe"), || {
                    panic!("invalid paths must fail before inventory lookup")
                })
                .unwrap_err()
                .code(),
                "application.unsupported_import"
            );
        }

        #[test]
        fn import_accepts_one_local_executable_or_unambiguous_bounded_directory() {
            let tree = Temp::new();
            let path = tree.file("Chat.exe");
            tree.file("ChatUpdater.exe");
            tree.file("notes.txt");
            let direct = import_path(&path).unwrap();
            assert_eq!(direct.discovery_source, DiscoverySource::ManualImport);
            assert_eq!(direct.platform, Platform::Windows);
            assert_eq!(direct.launch_target, format!("exe:{}", path.display()));
            assert_eq!(direct.display_name, "Chat");
            assert!(direct.signature_identity.starts_with("winfile-v1:"));
            assert_eq!(direct, import_path(&tree.0).unwrap());
            let nested = tree.0.join("bin");
            fs::create_dir(&nested).unwrap();
            let moved = nested.join("Chat.exe");
            fs::rename(&path, &moved).unwrap();
            assert_eq!(
                import_path(&tree.0).unwrap().launch_target,
                format!("exe:{}", moved.display())
            );
        }

        #[test]
        fn import_rejects_scripts_missing_relative_helpers_ambiguity_and_protected_paths() {
            let tree = Temp::new();
            for name in [
                "Chat.bat",
                "Chat.cmd",
                "Chat.ps1",
                "ChatUpdater.exe",
                "powershell.exe",
            ] {
                assert_eq!(
                    import_path(&tree.file(name)).unwrap_err().code(),
                    "application.unsupported_import"
                );
            }
            for path in [
                Path::new("Chat.exe"),
                Path::new(r"C:\missing-logindeck-test.exe"),
                Path::new(r"C:\Program Files\WindowsApps"),
                Path::new(r"C:\Program Files\WINDOWSAPPS\Package\App.exe"),
            ] {
                assert!(import_path(path).is_err());
            }
            assert!(import_path(&tree.0).is_err());
            tree.file("Chat.exe");
            tree.file("Editor.exe");
            assert!(import_path(&tree.0).is_err());
            let protected = tree.0.join("WindowsApps");
            fs::create_dir(&protected).unwrap();
            fs::write(protected.join("App.exe"), b"fixture").unwrap();
            assert!(import_path(&protected).is_err());
            assert!(import_path(&protected.join("App.exe")).is_err());
        }

        #[test]
        fn import_rejects_count_and_depth_overflow_even_with_one_observed_executable() {
            let tree = Temp::new();
            tree.file("Chat.exe");
            for i in 0..256 {
                tree.file(&format!("f{i}.txt"));
            }
            assert!(import_path(&tree.0).is_err());
            let deep = Temp::new();
            deep.file("Chat.exe");
            fs::create_dir_all(deep.0.join("one/two/three")).unwrap();
            assert!(import_path(&deep.0).is_err());
        }

        #[test]
        fn import_rejects_junction_root_and_nested_junction_without_traversing() {
            let tree = Temp::new();
            let external = Temp::new();
            external.file("External.exe");
            let junction = tree.0.join("link");
            // Junction creation is unprivileged; arguments are passed separately.
            let status = Command::new("cmd")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(&junction)
                .arg(&external.0)
                .output()
                .unwrap();
            assert!(status.status.success(), "test junction creation failed");
            assert!(import_path(&junction).is_err());
            assert!(import_path(&junction.join("External.exe")).is_err());
            assert!(import_path(&tree.0).is_err());
            fs::remove_dir(&junction).unwrap();
            assert!(external.0.join("External.exe").is_file());
        }
    }
}
