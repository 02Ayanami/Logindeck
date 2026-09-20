//! Windows platform integration primitives for LoginDeck.

pub(crate) mod app_catalog;
pub mod clipboard;
pub mod credentials;
mod data_directory;
pub mod error;
mod icons;
mod launch;
pub mod target;

pub use app_catalog::WindowsApplicationCatalog;
pub use clipboard::{WindowsClipboard, WindowsClipboardChangeToken};
pub use credentials::WindowsCredentialStore;
pub use data_directory::application_data_directory;
pub use error::{credential_error, credential_last_status, record_last_status};
pub use icons::{application_icon, application_icon_for_record};
pub use target::WindowsLaunchTarget;
