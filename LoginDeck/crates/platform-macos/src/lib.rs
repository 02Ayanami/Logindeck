//! macOS platform integration primitives for AutoLogin.

mod app_catalog;
mod bookmarks;
mod clipboard;
mod credentials;
mod icons;
pub mod login;

pub use app_catalog::MacApplicationCatalog;
pub use clipboard::{MacClipboard, MacPasteboardChangeToken};
pub use credentials::{keychain_last_status, MacCredentialStore};
pub use icons::application_icon;
