//! Private launch implementation. No shell, argument, credential, or fallback entry point.
use crate::{
    app_catalog::{package, win32, WindowsAppCandidate},
    WindowsLaunchTarget,
};
use autologin_core::{AppError, ApplicationRecord, DiscoverySource, Platform, VerifiedApplication};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::path::Path;

fn unsupported() -> AppError {
    AppError::new("application.unsupported_target")
}
fn failed() -> AppError {
    AppError::new("application.launch_failed")
}

fn text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(char::is_control)
}
fn decode(value: &str, max: usize) -> Option<String> {
    if value.len() > max.div_ceil(3) * 4 {
        return None;
    }
    let result = String::from_utf8(URL_SAFE_NO_PAD.decode(value).ok()?).ok()?;
    (text(&result, max) && URL_SAFE_NO_PAD.encode(&result) == value).then_some(result)
}

pub(crate) fn validated_target(app: &ApplicationRecord) -> Result<WindowsLaunchTarget, AppError> {
    if app.platform != Platform::Windows
        || app.path_access_ref.is_some()
        || !text(&app.display_name, 256)
        || !text(&app.platform_application_id, 512)
        || !text(&app.launch_target, 4096)
        || !text(&app.signature_identity, 16 * 1024)
        || app.version.as_ref().is_some_and(|v| !text(v, 256))
        || app.icon_cache_ref.as_ref().is_some_and(|v| !text(v, 4096))
        || [&app.last_discovered_at, &app.created_at, &app.updated_at]
            .iter()
            .any(|v| !text(v, 128))
        || app.alternate_launch_targets.len() > 32
        || app
            .alternate_launch_targets
            .iter()
            .any(|v| !text(v, 4096) || WindowsLaunchTarget::parse(v).is_err())
        || autologin_core::validate_application_metadata(
            &app.display_name,
            &app.platform_application_id,
            &app.launch_target,
            &app.alternate_launch_targets,
            app.version.as_deref(),
            &app.signature_identity,
            None,
        )
        .is_err()
    {
        return Err(unsupported());
    }
    let target = WindowsLaunchTarget::parse(&app.launch_target).map_err(|_| unsupported())?;
    let valid = match &target {
        WindowsLaunchTarget::Executable(path) => {
            let value = path.to_str().ok_or_else(unsupported)?;
            let id = app
                .platform_application_id
                .strip_prefix("win32:")
                .unwrap_or_default();
            let signature: Vec<_> = app.signature_identity.split(':').collect();
            win32::local_path(value)
                && !win32::is_command_host(value)
                && id.len() == 32
                && id
                    .bytes()
                    .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
                && signature.len() == 6
                && signature[0] == "winfile-v1"
                && decode(signature[1], 4000)
                    .is_some_and(|v| win32::local_path(&v) && v == win32::path_key(path))
                && signature[2..].iter().zip([8, 16, 16, 16]).all(|(v, len)| {
                    v.len() == len
                        && v.bytes()
                            .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
                })
        }
        WindowsLaunchTarget::Aumid(aumid) => {
            let signature: Vec<_> = app.signature_identity.split(':').collect();
            package::valid_aumid(aumid)
                && app.platform_application_id == *aumid
                && signature.len() == 3
                && signature[0] == "package-v1"
                && decode(signature[1], 512).is_some_and(|v| {
                    !v.is_empty()
                        && v.bytes()
                            .all(|x| x.is_ascii_alphanumeric() || matches!(x, b'.' | b'_' | b'-'))
                })
                && decode(signature[2], 512).as_deref() == Some(aumid.as_str())
        }
    };
    if !valid {
        return Err(unsupported());
    }
    Ok(target)
}

fn verify_package_with(
    app: &ApplicationRecord,
    enumerate: impl FnOnce() -> Result<Vec<WindowsAppCandidate>, AppError>,
    activate: impl FnOnce(&str) -> Result<u32, AppError>,
) -> Result<u32, AppError> {
    let WindowsLaunchTarget::Aumid(aumid) = validated_target(app)? else {
        return Err(unsupported());
    };
    let current = enumerate()
        .map_err(|_| failed())?
        .into_iter()
        .take(crate::app_catalog::MAX_SOURCE_ITEMS)
        .find(|item| {
            item.identity == aumid && item.target == WindowsLaunchTarget::Aumid(aumid.clone())
        })
        .ok_or_else(AppError::application_not_found)?;
    if current.signature_identity != app.signature_identity {
        return Err(AppError::application_signature_changed());
    }
    activate(&aumid)
        .map_err(|_| failed())
        .and_then(|pid| if pid == 0 { Err(failed()) } else { Ok(pid) })
}

#[cfg(windows)]
pub(crate) fn verify_and_launch(app: ApplicationRecord) -> Result<VerifiedApplication, AppError> {
    let target = validated_target(&app)?;
    let (pid, target) = match target {
        WindowsLaunchTarget::Executable(path) => native::win32(&app, &path)?,
        WindowsLaunchTarget::Aumid(_) => {
            let pid = verify_package_with(
                &app,
                || package::PackagedAppInventory.enumerate_current_user(),
                native::activate,
            )?;
            (pid, target)
        }
    };
    Ok(VerifiedApplication {
        application: app,
        verified_launch_target: target.encode(),
        launched_process_id: pid,
    })
}

#[cfg(not(windows))]
pub(crate) fn verify_and_launch(_app: ApplicationRecord) -> Result<VerifiedApplication, AppError> {
    Err(unsupported())
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::{
                Com::{
                    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER,
                    COINIT_MULTITHREADED,
                },
                Threading::{
                    CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW,
                },
            },
            UI::Shell::{ApplicationActivationManager, IApplicationActivationManager, AO_NONE},
        },
    };
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                let _ = unsafe { CloseHandle(self.0) };
            }
        }
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    pub(super) fn win32(
        app: &ApplicationRecord,
        path: &Path,
    ) -> Result<(u32, WindowsLaunchTarget), AppError> {
        // Opaque registry IDs must still name the same current candidate. Manual
        // IDs can be recomputed without registry availability. The current source
        // also preserves Task 5's explicit user-facing shortcut helper exception.
        let manual = app.discovery_source == DiscoverySource::ManualImport
            && app.platform_application_id == win32::identity_key("manual", path);
        if manual {
            if win32::is_helper(path.to_str().ok_or_else(unsupported)?) {
                return Err(unsupported());
            }
        } else {
            let candidates = win32::Win32Inventory.enumerate().map_err(|_| failed())?;
            let candidate = candidates.into_iter().find(|item| {
                (item.identity == app.platform_application_id || item.alternate_identities.contains(&app.platform_application_id))
                    && matches!(&item.target, WindowsLaunchTarget::Executable(p) if win32::path_key(p) == win32::path_key(path))
            }).ok_or_else(AppError::application_not_found)?;
            if candidate.signature_identity != app.signature_identity {
                return Err(AppError::application_signature_changed());
            }
        }
        verified_file_with(app, path, manual, create_process)
    }

    pub(super) fn verified_file_with(
        app: &ApplicationRecord,
        path: &Path,
        manual: bool,
        create: impl FnOnce(&Path) -> Result<u32, AppError>,
    ) -> Result<(u32, WindowsLaunchTarget), AppError> {
        // Pin local components and the final file (GENERIC_READ, read sharing only)
        // through CreateProcess. This recomputes the exact Task 5 metadata identity.
        let checked = win32::filesystem::checked_file(path.to_str().ok_or_else(unsupported)?)
            .ok_or_else(AppError::application_not_found)?;
        let value = checked.path.to_str().ok_or_else(unsupported)?;
        if win32::is_command_host(value)
            || (manual && win32::is_helper(value))
            || !checked
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            return Err(unsupported());
        }
        if checked.fingerprint().as_deref() != Some(app.signature_identity.as_str()) {
            return Err(AppError::application_signature_changed());
        }
        let pid = create(&checked.path)?;
        if pid == 0 {
            return Err(failed());
        }
        Ok((pid, WindowsLaunchTarget::Executable(checked.path.clone())))
    }

    fn create_process(path: &Path) -> Result<u32, AppError> {
        let value = path.to_str().ok_or_else(unsupported)?;
        let wide: Vec<_> = value.encode_utf16().chain([0]).collect();
        // local_path excludes quotes and NUL; exactly one quoted argv[0], no arguments.
        let mut command: Vec<_> = format!("\"{value}\"").encode_utf16().chain([0]).collect();
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        let result = unsafe {
            CreateProcessW(
                PCWSTR(wide.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS(0),
                None,
                PCWSTR::null(),
                &startup,
                &mut process,
            )
        };
        let _process = Handle(process.hProcess);
        let _thread = Handle(process.hThread);
        result.map_err(|_| failed())?;
        if process.dwProcessId == 0 {
            return Err(failed());
        }
        Ok(process.dwProcessId)
    }

    pub(super) fn activate(aumid: &str) -> Result<u32, AppError> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|_| failed())?;
        let _apartment = Apartment;
        let manager: IApplicationActivationManager =
            unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER) }
                .map_err(|_| failed())?;
        let wide: Vec<_> = aumid.encode_utf16().chain([0]).collect();
        unsafe { manager.ActivateApplication(PCWSTR(wide.as_ptr()), PCWSTR::null(), AO_NONE) }
            .map_err(|_| failed())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::app_catalog::WindowsAppCandidate;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use std::cell::Cell;

    #[test]
    fn final_file_guard_spans_the_process_creation_callback() {
        let root =
            std::env::temp_dir().join(format!("logindeck-launch-pin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let path = root.join("Fixture.exe");
        std::fs::write(&path, b"controlled inert fixture").unwrap();
        let exe = win32::filesystem::executable(path.to_str().unwrap()).unwrap();
        let mut app = ApplicationRecord::new(
            Platform::Windows,
            win32::identity_key("manual", &exe.path),
            "Fixture".into(),
        )
        .unwrap();
        app.launch_target = WindowsLaunchTarget::Executable(exe.path).encode();
        app.signature_identity = exe.fingerprint;
        app.discovery_source = DiscoverySource::ManualImport;
        let pid = native::verified_file_with(&app, &path, true, |canonical| {
            assert_eq!(win32::path_key(canonical), win32::path_key(&path));
            assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
            assert!(std::fs::rename(&path, root.join("replacement.exe")).is_err());
            Ok(417)
        })
        .unwrap();
        assert_eq!(pid.0, 417);
        std::fs::write(&path, b"writes resume after guard release").unwrap();
        let error = native::verified_file_with(&app, &path, true, |_| {
            panic!("changed file must not reach process creation")
        })
        .unwrap_err();
        assert_eq!(error.code(), "application.signature_changed");
    }

    #[test]
    fn manual_helpers_and_tampered_winfile_fields_do_not_reach_launch() {
        let path = Path::new(r"C:\Fixture\ChatUpdater.exe");
        let mut app = ApplicationRecord::new(
            Platform::Windows,
            win32::identity_key("manual", path),
            "Fixture".into(),
        )
        .unwrap();
        app.launch_target = format!("exe:{}", path.display());
        app.signature_identity = format!(
            "winfile-v1:{}:12345678:0000000000000001:0000000000000002:0000000000000003",
            URL_SAFE_NO_PAD.encode(win32::path_key(path))
        );
        app.discovery_source = DiscoverySource::ManualImport;
        assert_eq!(
            verify_and_launch(app.clone()).unwrap_err().code(),
            "application.unsupported_target"
        );
        for signature in [
            "winfile-v2:unknown".to_owned(),
            app.signature_identity.replace("12345678", "1234567G"),
            app.signature_identity.replace("0000000000000001", "1"),
            format!("{}:extra", app.signature_identity),
            app.signature_identity.replacen(
                &URL_SAFE_NO_PAD.encode(win32::path_key(path)),
                &URL_SAFE_NO_PAD.encode(r"c:\other.exe"),
                1,
            ),
        ] {
            let mut invalid = app.clone();
            invalid.signature_identity = signature;
            assert_eq!(
                validated_target(&invalid).unwrap_err().code(),
                "application.unsupported_target"
            );
        }
    }

    fn package() -> (ApplicationRecord, WindowsAppCandidate) {
        let aumid = "Contoso.Chat_abc!App";
        let signature = format!(
            "package-v1:{}:{}",
            URL_SAFE_NO_PAD.encode("Contoso.Chat_1.0.0.0_x64__abc"),
            URL_SAFE_NO_PAD.encode(aumid)
        );
        let mut app =
            ApplicationRecord::new(Platform::Windows, aumid.into(), "Fixture".into()).unwrap();
        app.launch_target = format!("aumid:{aumid}");
        app.signature_identity = signature.clone();
        let candidate = WindowsAppCandidate {
            identity: aumid.into(),
            display_name: "Fixture".into(),
            version: None,
            target: WindowsLaunchTarget::Aumid(aumid.into()),
            icon_sources: vec![],
            signature_identity: signature,
            executable_identity: None,
            sources: vec![],
            alternate_identities: vec![],
        };
        (app, candidate)
    }

    #[test]
    fn package_launch_requires_exact_entry_and_signature_before_activation() {
        let (app, candidate) = package();
        let called = Cell::new(0);
        let pid = verify_package_with(
            &app,
            || Ok(vec![candidate.clone()]),
            |aumid| {
                assert_eq!(aumid, "Contoso.Chat_abc!App");
                called.set(called.get() + 1);
                Ok(417)
            },
        )
        .unwrap();
        assert_eq!(pid, 417);
        assert_eq!(called.get(), 1);
        for (records, code) in [
            (vec![], "application.not_found"),
            (
                {
                    let mut x = candidate.clone();
                    x.identity = "Contoso.Chat_abc!Other".into();
                    x.target = WindowsLaunchTarget::Aumid(x.identity.clone());
                    vec![x]
                },
                "application.not_found",
            ),
            (
                {
                    let mut x = candidate.clone();
                    x.signature_identity.push('x');
                    vec![x]
                },
                "application.signature_changed",
            ),
        ] {
            let error = verify_package_with(&app, || Ok(records), |_| panic!("must not activate"))
                .unwrap_err();
            assert_eq!(error.code(), code);
            assert!(error.params.is_empty());
        }
        let error = verify_package_with(
            &app,
            || Err(AppError::new("private OS detail")),
            |_| panic!("must not activate"),
        )
        .unwrap_err();
        assert_eq!(error.code(), "application.launch_failed");
        assert!(error.params.is_empty());
        for result in [Ok(0), Err(AppError::new("private activation status"))] {
            let error =
                verify_package_with(&app, || Ok(vec![candidate.clone()]), |_| result).unwrap_err();
            assert_eq!(error.code(), "application.launch_failed");
            assert!(error.params.is_empty());
        }
    }

    #[test]
    fn untrusted_records_are_rejected_before_native_callbacks() {
        for field in 0..10 {
            let (mut app, _) = package();
            match field {
                0 => app.platform = Platform::Macos,
                1 => app.launch_target = "powershell.exe -c calc".into(),
                2 => app.launch_target = "aumid:Contoso.Chat_abc!App --payload".into(),
                3 => app.platform_application_id = "Contoso.Chat_abc!Other".into(),
                4 => app.signature_identity = "package-v2:unknown".into(),
                5 => app.signature_identity = "x".repeat(16385),
                6 => {
                    app.signature_identity = format!(
                        "package-v1:{}:{}",
                        URL_SAFE_NO_PAD.encode("Contoso.Chat_1.0.0.0_x64__abc"),
                        URL_SAFE_NO_PAD.encode("Contoso.Chat_abc!Other")
                    )
                }
                7 => app.display_name = "bad\0name".into(),
                8 => app.path_access_ref = Some("untrusted bookmark".into()),
                _ => app.alternate_launch_targets = vec!["cmd /c calc".into()],
            }
            let error = verify_package_with(
                &app,
                || panic!("must validate before enumeration"),
                |_| panic!("must not activate"),
            )
            .unwrap_err();
            assert_eq!(error.code(), "application.unsupported_target");
            assert!(error.params.is_empty());
        }
    }
}
