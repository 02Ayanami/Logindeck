//! Target-selected platform facade.
//!
//! Product code depends on this crate instead of importing a concrete platform crate. The domain,
//! frontend, and desktop command surface remain shared.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("LoginDeck supports only macOS and Windows platform runtimes");

#[cfg(target_os = "macos")]
mod selected {
    pub use platform_macos::{
        application_data_directory, application_icon,
        keychain_last_status as credential_last_status, MacApplicationCatalog, MacClipboard,
        MacCredentialStore,
    };

    pub type NativeApplicationCatalog = MacApplicationCatalog;
    pub type NativeClipboard = MacClipboard;
    pub type NativeCredentialStore = MacCredentialStore;

    pub const CREDENTIAL_BACKEND: &str = "login_keychain";
    pub const PLATFORM: autologin_core::Platform = autologin_core::Platform::Macos;

    /// Selects a native bundle path; the icon extractor verifies existence before reading it.
    pub fn application_icon_path(
        platform: autologin_core::Platform,
        target: &str,
    ) -> Option<std::path::PathBuf> {
        let path = std::path::Path::new(target);
        (platform == PLATFORM
            && path.is_absolute()
            && path.extension().is_some_and(|ext| ext == "app"))
        .then(|| path.to_path_buf())
    }

    /// macOS icons retain their main-thread requirement and bundle-path behavior.
    pub fn application_icon_for_record(app: &autologin_core::ApplicationRecord) -> Option<String> {
        if app.platform != PLATFORM {
            return None;
        }
        application_icon(std::path::Path::new(&app.launch_target))
    }

    pub mod login {
        pub use platform_macos::login::*;
    }
}

#[cfg(target_os = "windows")]
mod selected {
    pub use platform_windows::{
        application_data_directory, application_icon, application_icon_for_record,
        credential_last_status, WindowsApplicationCatalog, WindowsClipboard,
        WindowsCredentialStore,
    };

    pub type NativeApplicationCatalog = WindowsApplicationCatalog;
    pub type NativeClipboard = WindowsClipboard;
    pub type NativeCredentialStore = WindowsCredentialStore;

    pub const CREDENTIAL_BACKEND: &str = "windows_credential_manager";
    pub const PLATFORM: autologin_core::Platform = autologin_core::Platform::Windows;

    /// Decodes only native executable targets. Native extraction revalidates the local file;
    /// package identifiers are never converted to paths and may use the record facade instead.
    pub fn application_icon_path(
        platform: autologin_core::Platform,
        target: &str,
    ) -> Option<std::path::PathBuf> {
        if platform != PLATFORM {
            return None;
        }
        match platform_windows::WindowsLaunchTarget::parse(target).ok()? {
            platform_windows::WindowsLaunchTarget::Executable(path) => {
                use std::path::{Component, Prefix};
                matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
                    .then_some(path)
            }
            platform_windows::WindowsLaunchTarget::Aumid(_) => None,
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use selected::*;

#[cfg(test)]
mod tests {
    #[test]
    fn application_data_directory_is_absolute_and_uses_fixed_identifier() {
        let path = super::application_data_directory().expect("application data directory");

        assert!(path.is_absolute());
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("com.autologin.desktop")
        );
    }
}
