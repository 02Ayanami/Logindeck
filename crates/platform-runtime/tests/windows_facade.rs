#![cfg(target_os = "windows")]

use autologin_core::{ApplicationCatalog, ApplicationRecord, Clipboard, CredentialStore, Platform};
use platform_runtime::{
    application_icon, application_icon_for_record, credential_last_status,
    NativeApplicationCatalog, NativeClipboard, NativeCredentialStore, CREDENTIAL_BACKEND, PLATFORM,
};

#[test]
fn windows_aliases_implement_the_shared_contracts() {
    fn catalog(_: impl ApplicationCatalog) {}
    fn clipboard(_: impl Clipboard) {}
    fn credentials(_: impl CredentialStore) {}
    catalog(NativeApplicationCatalog::new());
    clipboard(NativeClipboard::new());
    credentials(NativeCredentialStore::new("runtime-contract".into()));
    assert_eq!(PLATFORM, Platform::Windows);
    assert_eq!(CREDENTIAL_BACKEND, "windows_credential_manager");
    let _: Option<i32> = credential_last_status();
}

#[test]
fn icon_facades_accept_records_and_reject_missing_or_untrusted_targets() {
    let mut record = ApplicationRecord::new(
        Platform::Windows,
        "LoginDeck.Missing_abc!App".into(),
        "Missing fixture".into(),
    )
    .unwrap();
    record.launch_target = "aumid:LoginDeck.Missing_abc!App".into();
    // Missing identity must fail closed; a typed package target is never a file path.
    assert_eq!(application_icon_for_record(&record), None);
    assert_eq!(application_icon(std::path::Path::new("relative.exe")), None);
}

#[test]
fn scan_icon_paths_decode_only_native_executable_targets() {
    use platform_runtime::application_icon_path;
    use std::path::PathBuf;
    assert_eq!(
        application_icon_path(Platform::Windows, r"exe:C:\Apps\Editor.exe"),
        Some(PathBuf::from(r"C:\Apps\Editor.exe"))
    );
    for target in [
        "aumid:Contoso.Chat_abc!App",
        "exe:relative.exe",
        r"exe:\\server\share\App.exe",
        "cmd.exe /c calc",
        "",
    ] {
        assert_eq!(
            application_icon_path(Platform::Windows, target),
            None,
            "{target}"
        );
    }
    assert_eq!(
        application_icon_path(Platform::Macos, "/Applications/Editor.app"),
        None
    );
}
