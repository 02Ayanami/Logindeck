//! Current-user registered launchable applications; no manifest filesystem walk.
use super::{WindowsAppCandidate, MAX_SOURCE_ITEMS};
use crate::WindowsLaunchTarget;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

struct PackageRecord {
    family: String,
    full_name: String,
    version: String,
    framework: bool,
    resource: bool,
    available: bool,
    applications: Vec<ApplicationEntry>,
}

#[derive(Clone)]
struct ApplicationEntry {
    id: String,
    aumid: String,
    display_name: Option<String>,
    executable_identity: Option<String>,
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'_' | b'-'))
}

pub(crate) fn valid_aumid(value: &str) -> bool {
    value.split_once('!').is_some_and(|(family, app)| {
        identifier(family, 255) && family.contains('_') && identifier(app, 64)
    })
}

fn presentation(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value.chars().any(char::is_control)
        && !value.to_ascii_lowercase().starts_with("ms-resource:")
}

fn verified_overlap(
    aumid: &str,
    shell_aumid: &str,
    executable: super::win32::SafeExecutable,
    roots: &[std::path::PathBuf],
) -> Option<String> {
    // A shared system host outside the package cannot prove a desktop overlap.
    (aumid.eq_ignore_ascii_case(shell_aumid)
        && roots
            .iter()
            .any(|root| super::win32::within(&executable.path, root)))
    .then_some(executable.fingerprint)
}

fn normalize_package(package: PackageRecord) -> Vec<WindowsAppCandidate> {
    if package.framework
        || package.resource
        || !package.available
        || !identifier(&package.family, 255)
        || !package.family.contains('_')
        || !identifier(&package.full_name, 512)
    {
        return vec![];
    }
    package
        .applications
        .into_iter()
        .take(MAX_SOURCE_ITEMS)
        .filter_map(|app| {
            if !identifier(&app.id, 64)
                || app.aumid.len() > 512
                || app.aumid != format!("{}!{}", package.family, app.id)
            {
                return None;
            }
            let display_name = app
                .display_name
                .filter(|name| presentation(name, 256))
                .unwrap_or(app.id);
            // Full registration name binds version/architecture/resource/publisher-id;
            // AUMID binds the family and Application. Neither is publisher/content trust.
            let signature_identity = format!(
                "package-v1:{}:{}",
                URL_SAFE_NO_PAD.encode(&package.full_name),
                URL_SAFE_NO_PAD.encode(&app.aumid)
            );
            Some(WindowsAppCandidate {
                identity: app.aumid.clone(),
                display_name,
                version: presentation(&package.version, 256).then(|| package.version.clone()),
                target: WindowsLaunchTarget::Aumid(app.aumid),
                // Logos are resolved lazily from AppListEntry.DisplayInfo in Task 7.
                icon_sources: vec![],
                signature_identity,
                executable_identity: app.executable_identity.filter(|value| {
                    value.starts_with("winfile-v1:") && presentation(value, 16 * 1024)
                }),
                sources: vec!["current-user-package".into()],
                alternate_identities: vec![],
            })
        })
        .collect()
}

pub(crate) struct PackagedAppInventory;
#[cfg(windows)]
pub(crate) fn with_logo<T>(
    aumid: &str,
    signature: &str,
    resolve: impl FnOnce(windows::Storage::Streams::RandomAccessStreamReference) -> Option<T>,
) -> Option<T> {
    native::with_logo(aumid, signature, resolve)
}
impl PackagedAppInventory {
    pub(crate) fn enumerate_current_user(
        &self,
    ) -> Result<Vec<WindowsAppCandidate>, autologin_core::AppError> {
        #[cfg(windows)]
        {
            native::enumerate()
        }
        #[cfg(not(windows))]
        {
            Err(autologin_core::AppError::new(
                "application.discovery_unavailable",
            ))
        }
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::{
        core::{HSTRING, PCWSTR, PWSTR},
        ApplicationModel::Package,
        Management::Deployment::PackageManager,
        Win32::{
            Foundation::PROPERTYKEY,
            Storage::EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_Link_TargetParsingPath},
            System::{
                Com::CoTaskMemFree,
                WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
            },
            UI::Shell::{IShellItem2, SHCreateItemFromParsingName},
        },
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    fn bounded(value: HSTRING, max: usize) -> Option<String> {
        if value.len() > max {
            return None;
        }
        let text = String::from_utf16(&value).ok()?;
        (text.len() <= max && !text.chars().any(char::is_control)).then_some(text)
    }

    struct ShellString(PWSTR);
    impl Drop for ShellString {
        fn drop(&mut self) {
            unsafe { CoTaskMemFree(Some(self.0 .0.cast())) };
        }
    }
    fn shell_string(item: &IShellItem2, key: &PROPERTYKEY, max: usize) -> Option<String> {
        let text = ShellString(unsafe { item.GetString(key) }.ok()?);
        if text.0.is_null() {
            return None;
        }
        // GetString owns a NUL-terminated CoTaskMem allocation. Bound traversal
        // before allocating Rust strings, and always release the native string.
        for len in 0..=max {
            if unsafe { *text.0 .0.add(len) } == 0 {
                let value =
                    String::from_utf16(unsafe { std::slice::from_raw_parts(text.0 .0, len) })
                        .ok()?;
                return (value.len() <= max).then_some(value);
            }
        }
        None
    }

    fn executable_identity(aumid: &str, roots: &[std::path::PathBuf]) -> Option<String> {
        // Ask the OS AppsFolder item for this exact registered AUMID. This is
        // optional overlap evidence, never a guessed path or a relaunch command.
        let parsing: Vec<_> = format!("shell:AppsFolder\\{aumid}")
            .encode_utf16()
            .chain([0])
            .collect();
        let item: IShellItem2 =
            unsafe { SHCreateItemFromParsingName(PCWSTR(parsing.as_ptr()), None) }.ok()?;
        let shell_aumid = shell_string(&item, &PKEY_AppUserModel_ID, 512)?;
        let path = shell_string(&item, &PKEY_Link_TargetParsingPath, 4000)?;
        let executable = super::super::win32::filesystem::executable(&path)?;
        verified_overlap(aumid, &shell_aumid, executable, roots)
    }

    fn record(package: &Package, remaining: &mut usize) -> Option<PackageRecord> {
        if package.IsFramework().ok()?
            || package.IsResourcePackage().ok()?
            || !package.Status().ok()?.VerifyIsOK().ok()?
            || package.IsStub().ok()?
        {
            return None;
        }
        let identity = package.Id().ok()?;
        let family = bounded(identity.FamilyName().ok()?, 255)?;
        let full_name = bounded(identity.FullName().ok()?, 512)?;
        if !identifier(&family, 255) || !identifier(&full_name, 512) {
            return None;
        }
        let version = identity
            .Version()
            .ok()
            .map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision))
            .unwrap_or_default();
        // OS-declared locations, including sparse/external desktop packages.
        // Only metadata is requested; no directory or manifest is traversed.
        let roots: Vec<_> = [
            package.InstalledPath().ok(),
            package.EffectiveExternalPath().ok(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|path| bounded(path, 4000))
        .filter(|path| super::super::win32::local_path(path))
        .map(std::path::PathBuf::from)
        .collect();
        // Synchronous public API available since Windows 10 2004, before our
        // minimum 22H2. It exposes actual launchable entries, including multi-app packages.
        let entries = package.GetAppListEntries().ok()?;
        let count = (entries.Size().ok()? as usize).min(*remaining);
        let mut applications = Vec::new();
        for index in 0..count {
            *remaining -= 1; // Broken entries consume the same global raw budget.
            let Some(app) = (|| {
                let entry = entries.GetAt(index as u32).ok()?;
                let aumid = bounded(entry.AppUserModelId().ok()?, 512)?;
                let (entry_family, id) = aumid.split_once('!')?;
                if entry_family != family || !identifier(id, 64) {
                    return None;
                }
                let display_name = entry
                    .DisplayInfo()
                    .ok()
                    .and_then(|info| info.DisplayName().ok())
                    .and_then(|name| bounded(name, 256));
                Some(ApplicationEntry {
                    id: id.into(),
                    executable_identity: executable_identity(&aumid, &roots),
                    aumid,
                    display_name,
                })
            })() else {
                continue;
            };
            applications.push(app);
        }
        Some(PackageRecord {
            family,
            full_name,
            version,
            framework: false,
            resource: false,
            available: true,
            applications,
        })
    }

    pub(super) fn enumerate() -> Result<Vec<WindowsAppCandidate>, autologin_core::AppError> {
        let error = || autologin_core::AppError::new("application.discovery_unavailable");
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|_| error())?;
        let _apartment = Apartment;
        let manager = PackageManager::new().map_err(|_| error())?;
        // windows-rs maps FindPackagesForUser(String.Empty) to this overload name.
        let packages = manager
            .FindPackagesByUserSecurityId(&HSTRING::new())
            .map_err(|_| error())?;
        let iterator = packages.First().map_err(|_| error())?;
        let mut candidates = Vec::new();
        let mut remaining = MAX_SOURCE_ITEMS;
        for index in 0..MAX_SOURCE_ITEMS {
            if !iterator.HasCurrent().map_err(|_| error())? {
                break;
            }
            if let Ok(package) = iterator.Current() {
                if let Some(record) = record(&package, &mut remaining) {
                    candidates.extend(normalize_package(record));
                }
            }
            if remaining == 0 || index + 1 == MAX_SOURCE_ITEMS {
                break;
            }
            if !iterator.MoveNext().map_err(|_| error())? {
                break;
            }
        }
        Ok(candidates)
    }

    pub(super) fn with_logo<T>(
        aumid: &str,
        signature: &str,
        resolve: impl FnOnce(windows::Storage::Streams::RandomAccessStreamReference) -> Option<T>,
    ) -> Option<T> {
        if !valid_aumid(aumid) || signature.len() > 16 * 1024 {
            return None;
        }
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.ok()?;
        let _apartment = Apartment;
        let manager = PackageManager::new().ok()?;
        let packages = manager.FindPackagesByUserSecurityId(&HSTRING::new()).ok()?;
        let iterator = packages.First().ok()?;
        let mut remaining = MAX_SOURCE_ITEMS;
        for index in 0..MAX_SOURCE_ITEMS {
            if !iterator.HasCurrent().ok()? {
                break;
            }
            if let Ok(package) = iterator.Current() {
                if let Some(record) = record(&package, &mut remaining) {
                    let matched = normalize_package(record)
                        .into_iter()
                        .any(|item| item.identity == aumid && item.signature_identity == signature);
                    if matched {
                        // Select this exact Application, not a package-wide logo. GetLogo
                        // resolves manifest/PRI scale, targetsize and language qualifiers in Windows.
                        let entries = package.GetAppListEntries().ok()?;
                        for i in 0..entries.Size().ok()?.min(MAX_SOURCE_ITEMS as u32) {
                            let Ok(entry) = entries.GetAt(i) else {
                                continue;
                            };
                            if bounded(entry.AppUserModelId().ok()?, 512).as_deref() == Some(aumid)
                            {
                                let logo = entry
                                    .DisplayInfo()
                                    .ok()?
                                    .GetLogo(windows::Foundation::Size {
                                        Width: 64.0,
                                        Height: 64.0,
                                    })
                                    .ok()?;
                                return resolve(logo);
                            }
                        }
                        return None;
                    }
                }
            }
            if remaining == 0 || index + 1 == MAX_SOURCE_ITEMS || !iterator.MoveNext().ok()? {
                break;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(value: &Value) -> PackageRecord {
        PackageRecord {
            family: value["family"].as_str().unwrap().into(),
            full_name: value["full_name"].as_str().unwrap().into(),
            version: value["version"].as_str().unwrap_or("1.0.0.0").into(),
            framework: value["framework"].as_bool().unwrap_or(false),
            resource: value["resource"].as_bool().unwrap_or(false),
            available: value["available"].as_bool().unwrap_or(true),
            applications: value["applications"]
                .as_array()
                .unwrap()
                .iter()
                .map(|app| ApplicationEntry {
                    id: app["id"].as_str().unwrap().into(),
                    aumid: app["aumid"].as_str().unwrap().into(),
                    display_name: app["name"].as_str().map(str::to_owned),
                    executable_identity: app["executable_identity"].as_str().map(str::to_owned),
                })
                .collect(),
        }
    }

    fn normal() -> PackageRecord {
        let cases: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/packages.json")).unwrap();
        fixture(&cases[0])
    }

    #[test]
    fn package_fixtures_keep_only_current_launchable_application_identities() {
        let cases: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/packages.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let parsed = normalize_package(fixture(case));
            let actual: Vec<_> = parsed.iter().map(|item| item.target.encode()).collect();
            let expected: Vec<_> = case["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            assert_eq!(actual, expected, "{}", case["case"]);
        }
        let parsed = [
            normalize_package(fixture(&cases[0])).remove(0),
            normalize_package(fixture(&cases[1])).remove(0),
        ];
        assert_eq!(
            parsed.iter().map(|x| x.target.encode()).collect::<Vec<_>>(),
            vec![
                "aumid:Contoso.Chat_abc!App",
                "aumid:Contoso.Tools_abc!Editor"
            ]
        );
    }

    #[test]
    fn malformed_and_oversized_identity_fields_are_skipped() {
        for field in 0..5 {
            let mut package = normal();
            match field {
                0 => package.family = "x".repeat(256),
                1 => package.full_name = "x".repeat(513),
                2 => package.applications[0].id = "x".repeat(65),
                3 => package.applications[0].aumid = "x".repeat(513),
                _ => package.family = "Contoso\\Chat_abc".into(),
            }
            assert!(normalize_package(package).is_empty());
        }
    }

    #[test]
    fn presentation_failure_degrades_and_signature_tracks_registration_not_display_name() {
        let baseline = normalize_package(normal()).remove(0);
        assert!(baseline.signature_identity.starts_with("package-v1:"));
        assert!(baseline.signature_identity.len() < 16 * 1024);
        for name in [
            None,
            Some("x".repeat(257)),
            Some("ms-resource:Name".into()),
            Some("\0".into()),
        ] {
            let mut package = normal();
            package.applications[0].display_name = name;
            package.applications[0].executable_identity = Some("x".repeat(16385));
            let actual = normalize_package(package).remove(0);
            assert_eq!(actual.display_name, "App");
            assert_eq!(actual.signature_identity, baseline.signature_identity);
            assert_eq!(actual.executable_identity, None);
            assert!(actual.icon_sources.is_empty());
        }
        let mut changed = normal();
        changed.full_name = "Contoso.Chat_2.0.0.0_x64__abc".into();
        assert_ne!(
            normalize_package(changed)[0].signature_identity,
            baseline.signature_identity
        );
    }

    #[test]
    fn executable_overlap_requires_exact_aumid_and_package_owned_path() {
        use crate::app_catalog::win32::SafeExecutable;
        use std::path::PathBuf;
        let roots = vec![PathBuf::from(r"C:\Packages\Contoso")];
        let executable = || SafeExecutable {
            path: r"C:\Packages\Contoso\Chat.exe".into(),
            fingerprint: "winfile-v1:verified-file".into(),
        };
        assert_eq!(
            verified_overlap(
                "Contoso.Chat_abc!App",
                "Contoso.Chat_abc!App",
                executable(),
                &roots
            ),
            Some("winfile-v1:verified-file".into())
        );
        assert!(verified_overlap(
            "Contoso.Chat_abc!App",
            "Contoso.Other_abc!App",
            executable(),
            &roots
        )
        .is_none());
        let mut host = executable();
        host.path = r"C:\Windows\System32\WWAHost.exe".into();
        assert!(
            verified_overlap("Contoso.Chat_abc!App", "Contoso.Chat_abc!App", host, &roots)
                .is_none()
        );
        let mut sibling = executable();
        sibling.path = r"C:\Packages\ContosoOther\Chat.exe".into();
        assert!(verified_overlap(
            "Contoso.Chat_abc!App",
            "Contoso.Chat_abc!App",
            sibling,
            &roots
        )
        .is_none());
        assert!(verified_overlap(
            "Contoso.Chat_abc!App",
            "Contoso.Chat_abc!App",
            executable(),
            &[]
        )
        .is_none());
    }

    #[test]
    fn application_record_budget_is_applied_before_filtering() {
        let mut package = normal();
        let valid = package.applications[0].clone();
        let mut invalid = valid.clone();
        invalid.id = "!invalid".into();
        package.applications = vec![invalid; 4096];
        package.applications.push(valid);
        assert!(normalize_package(package).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn live_current_user_package_source_is_available_and_bounded() {
        let items = PackagedAppInventory.enumerate_current_user().unwrap();
        assert!(items.len() <= 4096);
        assert!(items
            .iter()
            .all(|item| matches!(item.target, WindowsLaunchTarget::Aumid(_))));
        eprintln!(
            "current-user package source: {} launchable entries",
            items.len()
        );
    }
}
