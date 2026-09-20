use serde::{de, Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::AppError;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(WebsiteId);
uuid_id!(ApplicationId);
uuid_id!(ApplicationAccountId);
uuid_id!(LoginTemplateId);

/// The operating system that owns a platform application identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
}

/// The interface language selected by the user.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Locale {
    #[serde(rename = "en")]
    #[default]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
}

/// Closed set of non-sensitive local preferences. Add variants as future preferences are designed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceKey {
    UiLocale,
}

/// Value permitted for a matching preference key. It cannot represent arbitrary secret text.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PreferenceValue {
    UiLocale(Locale),
}

/// A non-sensitive, closed preference key/value pair.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SettingRecord {
    pub key: PreferenceKey,
    pub value: PreferenceValue,
}
impl SettingRecord {
    pub fn new(key: PreferenceKey, value: PreferenceValue) -> Self {
        Self { key, value }
    }
}

/// An opaque platform-specific secure-storage reference, not a secret value.
#[derive(Clone, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    pub fn new(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(AppError::invalid_field("secret_ref"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretRef(<opaque>)")
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        SecretRef::new(value).map_err(de::Error::custom)
    }
}

/// The metadata-only category of a Keychain item that must be retried later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretCleanupKind {
    Website,
    ApplicationAccount,
}
impl SecretCleanupKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Website => "website",
            Self::ApplicationAccount => "application_account",
        }
    }
}

/// Durable cleanup work. It contains an opaque secure-storage reference, never a secret value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingSecretCleanup {
    pub secret_ref: SecretRef,
    pub kind: SecretCleanupKind,
    pub owner_id: Option<String>,
    pub context: String,
    pub attempts: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    Manual,
    BrowserExtension,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    Automatic,
    ManualImport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebsiteRecord {
    pub account_name: String,
    pub id: WebsiteId,
    pub name: String,
    pub url: String,
    pub normalized_origin: String,
    pub username: String,
    pub password_credential_ref: SecretRef,
    pub capture_source: CaptureSource,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Metadata for an installed application. Platform-specific fields remain opaque to the core.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApplicationRecord {
    pub id: ApplicationId,
    pub platform: Platform,
    pub display_name: String,
    pub platform_application_id: String,
    pub launch_target: String,
    /// Other validated bundle locations with the same platform identity.  This is metadata,
    /// never an instruction to launch an application.
    pub alternate_launch_targets: Vec<String>,
    pub path_access_ref: Option<String>,
    pub version: Option<String>,
    pub signature_identity: String,
    pub discovery_source: DiscoverySource,
    pub is_present: bool,
    pub is_hidden: bool,
    pub icon_cache_ref: Option<String>,
    pub last_discovered_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
struct ApplicationRecordData {
    id: ApplicationId,
    platform: Platform,
    display_name: String,
    platform_application_id: String,
    launch_target: String,
    alternate_launch_targets: Vec<String>,
    path_access_ref: Option<String>,
    version: Option<String>,
    signature_identity: String,
    discovery_source: DiscoverySource,
    is_present: bool,
    is_hidden: bool,
    icon_cache_ref: Option<String>,
    last_discovered_at: String,
    created_at: String,
    updated_at: String,
}

impl<'de> Deserialize<'de> for ApplicationRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = ApplicationRecordData::deserialize(deserializer)?;
        validate_application_metadata(
            &data.display_name,
            &data.platform_application_id,
            &data.launch_target,
            &data.alternate_launch_targets,
            data.version.as_deref(),
            &data.signature_identity,
            data.path_access_ref.as_deref(),
        )
        .map_err(de::Error::custom)?;

        Ok(Self {
            id: data.id,
            platform: data.platform,
            display_name: data.display_name,
            platform_application_id: data.platform_application_id,
            launch_target: data.launch_target,
            alternate_launch_targets: data.alternate_launch_targets,
            path_access_ref: data.path_access_ref,
            version: data.version,
            signature_identity: data.signature_identity,
            discovery_source: data.discovery_source,
            is_present: data.is_present,
            is_hidden: data.is_hidden,
            icon_cache_ref: data.icon_cache_ref,
            last_discovered_at: data.last_discovered_at,
            created_at: data.created_at,
            updated_at: data.updated_at,
        })
    }
}

impl ApplicationRecord {
    pub fn new(
        platform: Platform,
        platform_application_id: String,
        display_name: String,
    ) -> Result<Self, AppError> {
        bounded_nonblank("display_name", &display_name, 256)?;
        bounded_nonblank("platform_application_id", &platform_application_id, 512)?;

        Ok(Self {
            id: ApplicationId::new(),
            platform,
            display_name,
            platform_application_id,
            launch_target: String::new(),
            alternate_launch_targets: Vec::new(),
            path_access_ref: None,
            version: None,
            signature_identity: String::new(),
            discovery_source: DiscoverySource::Automatic,
            is_present: true,
            is_hidden: false,
            icon_cache_ref: None,
            last_discovered_at: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    #[default]
    Password,
    Sms,
    /// Legacy storage compatibility only; saving and launching QR accounts is disabled.
    Qr,
}
impl LoginMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Sms => "sms",
            Self::Qr => "qr",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplicationAccount {
    pub login_method: LoginMethod,
    pub phone: Option<String>,
    pub id: ApplicationAccountId,
    pub application_id: ApplicationId,
    pub display_name: String,
    pub username: String,
    pub password_credential_ref: Option<SecretRef>,
    pub auto_submit_enabled: bool,
    pub last_login_status: Option<String>,
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A user-confirmed, non-sensitive description of an application's login window.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoginTemplateRecord {
    pub id: LoginTemplateId,
    pub application_id: ApplicationId,
    pub window_signature: String,
    pub window_size_class: String,
    pub username_rect: String,
    pub password_rect: String,
    pub submit_rect: String,
    pub provider_id: String,
    pub model_name: String,
    pub confidence_summary: String,
    pub user_confirmed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// An application candidate found by a platform implementation without launching it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiscoveredApplication {
    pub platform: Platform,
    pub display_name: String,
    pub platform_application_id: String,
    pub launch_target: String,
    pub alternate_launch_targets: Vec<String>,
    pub path_access_ref: Option<String>,
    pub version: Option<String>,
    pub signature_identity: String,
    pub discovery_source: DiscoverySource,
}

/// A launch target whose identity was checked by the platform implementation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerifiedApplication {
    pub application: ApplicationRecord,
    pub verified_launch_target: String,
    /// PID attested after NSWorkspace launched the process. Credential interaction must bind to it.
    pub launched_process_id: u32,
}

fn validate_nonblank(field: &'static str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::invalid_field(field));
    }
    Ok(())
}

/// Bound persisted path metadata so an untrusted catalog cannot make a row unbounded.
pub fn validate_launch_targets(values: &[String]) -> Result<(), AppError> {
    if values.len() > 32
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 4096)
    {
        return Err(AppError::invalid_field("paths"));
    }
    Ok(())
}
pub fn validate_application_metadata(
    display_name: &str,
    platform_application_id: &str,
    launch_target: &str,
    alternates: &[String],
    version: Option<&str>,
    signature_identity: &str,
    path_access_ref: Option<&str>,
) -> Result<(), AppError> {
    bounded_nonblank("display_name", display_name, 256)?;
    bounded_nonblank("platform_application_id", platform_application_id, 512)?;
    bounded_nonblank("launch_target", launch_target, 4096)?;
    if version.is_some_and(|value| value.len() > 256) {
        return Err(AppError::invalid_field("version"));
    }
    if signature_identity.len() > 16 * 1024 {
        return Err(AppError::invalid_field("signature_identity"));
    }
    if path_access_ref.is_some_and(|value| value.len() > 1024 * 1024) {
        return Err(AppError::invalid_field("path_access_ref"));
    }
    validate_launch_targets(alternates)
}
fn bounded_nonblank(field: &'static str, value: &str, max: usize) -> Result<(), AppError> {
    if value.len() > max {
        return Err(AppError::invalid_field(field));
    }
    validate_nonblank(field, value)
}
