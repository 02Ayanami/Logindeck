//! Platform-neutral domain primitives and interfaces for AutoLogin.
mod mutation_lock;

pub mod adapter;
mod domain;
mod error;
mod platform;
mod repository;
mod services;
mod sqlite;
pub mod switcher;

pub use domain::{
    validate_application_metadata, validate_launch_targets, ApplicationAccount,
    ApplicationAccountId, ApplicationId, ApplicationRecord, CaptureSource, DiscoveredApplication,
    DiscoverySource, Locale, LoginMethod, LoginTemplateId, LoginTemplateRecord,
    PendingSecretCleanup, Platform, PreferenceKey, PreferenceValue, SecretCleanupKind, SecretRef,
    SettingRecord, VerifiedApplication, WebsiteId, WebsiteRecord,
};
pub use error::AppError;
pub use platform::{ApplicationCatalog, Clipboard, CredentialStore, UserPresence};
pub use repository::{
    ApplicationAccountsRepository, ApplicationsRepository, BrowserCaptureSettings,
    LoginTemplatesRepository, PendingSecretCleanupRepository, SettingsRepository,
    WebsitesRepository,
};
pub use services::{
    copy_password, ApplicationsService, ClipboardSession, SaveApplicationAccount, SaveWebsite,
    VaultService,
};
pub use sqlite::SqliteRepositories;
