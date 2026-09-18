//! Target-selected platform facade.
//!
//! Product code depends on this crate instead of importing a concrete platform crate. A future
//! platform implementation is wired here, while the domain, frontend, and desktop command surface
//! remain shared.

#[cfg(not(target_os = "macos"))]
compile_error!("LoginDeck currently has a verified macOS platform runtime only");

#[cfg(target_os = "macos")]
mod selected {
    pub use platform_macos::{
        application_icon, keychain_last_status as credential_last_status, MacApplicationCatalog,
        MacClipboard, MacCredentialStore,
    };

    pub type NativeApplicationCatalog = MacApplicationCatalog;
    pub type NativeClipboard = MacClipboard;
    pub type NativeCredentialStore = MacCredentialStore;

    pub const CREDENTIAL_BACKEND: &str = "login_keychain";

    pub mod login {
        pub use platform_macos::login::*;
    }
}

pub use selected::*;

pub const PLATFORM: autologin_core::Platform = autologin_core::Platform::Macos;
