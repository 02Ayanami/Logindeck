//! Windows platform integration primitives for LoginDeck.

pub mod error;
pub mod target;

pub use error::{credential_error, credential_last_status, record_last_status};
pub use target::WindowsLaunchTarget;
