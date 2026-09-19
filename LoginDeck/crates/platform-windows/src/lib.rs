//! Windows platform integration primitives for LoginDeck.

pub(crate) mod app_catalog;
pub mod clipboard;
pub mod credentials;
pub mod error;
pub mod target;

pub use app_catalog::WindowsApplicationCatalog;
pub use clipboard::{WindowsClipboard, WindowsClipboardChangeToken};
pub use credentials::WindowsCredentialStore;
pub use error::{credential_error, credential_last_status, record_last_status};
pub use target::WindowsLaunchTarget;
