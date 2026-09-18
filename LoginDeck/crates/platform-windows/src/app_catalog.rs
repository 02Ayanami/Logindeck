//! Internal discovery building blocks shared by the Windows catalog tasks.
#![allow(dead_code)] // Consumed by the catalog facade in the following task.

pub(crate) mod shortcut;
pub(crate) mod win32;

use crate::WindowsLaunchTarget;
use std::path::PathBuf;

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
    pub(crate) sources: Vec<String>,
    pub(crate) alternate_identities: Vec<String>,
}
