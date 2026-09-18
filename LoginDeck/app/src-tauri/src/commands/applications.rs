use crate::{commands::CommandError, state::AppState};
use autologin_core::{
    ApplicationAccount, ApplicationAccountId, ApplicationId, ApplicationRecord,
    SaveApplicationAccount,
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub async fn remove_application(
    id: ApplicationId,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .applications
        .remove_application(id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn get_application_icon(
    id: ApplicationId,
    handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    // Only stored application IDs are accepted, never caller-supplied paths or file contents.
    let application = state.applications.get_application(id).await?;
    if !application.is_present {
        return Ok(None);
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    handle
        .run_on_main_thread(move || {
            let icon = platform_application_icon(std::path::Path::new(&application.launch_target));
            let _ = send.send(icon);
        })
        .map_err(|_| autologin_core::AppError::new("internal.error"))?;
    receive
        .await
        .map_err(|_| autologin_core::AppError::new("internal.error").into())
}

#[tauri::command]
pub async fn get_scan_candidate_icon(
    id: u64,
    token: usize,
    handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    let path = state.scan.candidate_path(id, token)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    handle
        .run_on_main_thread(move || {
            let icon = platform_application_icon(&path);
            let _ = send.send(icon);
        })
        .map_err(|_| autologin_core::AppError::new("internal.error"))?;
    receive
        .await
        .map_err(|_| autologin_core::AppError::new("internal.error").into())
}

fn platform_application_icon(path: &std::path::Path) -> Option<String> {
    platform_runtime::application_icon(path)
}

#[derive(Clone, Serialize)]
pub struct ApplicationAccountResponse {
    pub id: ApplicationAccountId,
    pub application_id: ApplicationId,
    pub display_name: String,
    pub username: String,
    #[serde(default)]
    pub login_method: autologin_core::LoginMethod,
    pub phone: Option<String>,
    pub auto_submit_enabled: bool,
    pub last_login_status: Option<String>,
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
impl From<ApplicationAccount> for ApplicationAccountResponse {
    fn from(x: ApplicationAccount) -> Self {
        Self {
            id: x.id,
            application_id: x.application_id,
            display_name: x.display_name,
            username: x.username,
            login_method: x.login_method,
            phone: x.phone,
            auto_submit_enabled: x.auto_submit_enabled,
            last_login_status: x.last_login_status,
            last_login_at: x.last_login_at,
            created_at: x.created_at,
            updated_at: x.updated_at,
        }
    }
}
#[derive(Clone, Serialize)]
pub struct ApplicationResponse {
    pub id: ApplicationId,
    pub platform: autologin_core::Platform,
    pub display_name: String,
    pub platform_application_id: String,
    pub launch_target: String,
    pub alternate_launch_targets: Vec<String>,
    pub version: Option<String>,
    pub discovery_source: autologin_core::DiscoverySource,
    pub is_present: bool,
    pub last_discovered_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub accounts: Vec<ApplicationAccountResponse>,
}
async fn response(
    state: &AppState,
    x: ApplicationRecord,
) -> Result<ApplicationResponse, CommandError> {
    validate_response_record(&x)?;
    let accounts = state
        .applications
        .list_accounts(x.id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(ApplicationResponse {
        id: x.id,
        platform: x.platform,
        display_name: x.display_name,
        platform_application_id: x.platform_application_id,
        launch_target: x.launch_target,
        alternate_launch_targets: x.alternate_launch_targets,
        version: x.version,
        discovery_source: x.discovery_source,
        is_present: x.is_present,
        last_discovered_at: x.last_discovered_at,
        created_at: x.created_at,
        updated_at: x.updated_at,
        accounts,
    })
}
fn validate_response_record(x: &ApplicationRecord) -> Result<(), CommandError> {
    autologin_core::validate_application_metadata(
        &x.display_name,
        &x.platform_application_id,
        &x.launch_target,
        &x.alternate_launch_targets,
        x.version.as_deref(),
        &x.signature_identity,
        x.path_access_ref.as_deref(),
    )
    .map_err(Into::into)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveApplicationAccountRequest {
    pub id: Option<ApplicationAccountId>,
    pub application_id: ApplicationId,
    pub display_name: String,
    pub username: String,
    #[serde(default)]
    pub login_method: autologin_core::LoginMethod,
    pub phone: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub auto_submit_enabled: bool,
}
impl std::fmt::Debug for SaveApplicationAccountRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveApplicationAccountRequest")
            .field("id", &self.id)
            .field("application_id", &self.application_id)
            .field("display_name", &self.display_name)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("auto_submit_enabled", &self.auto_submit_enabled)
            .finish()
    }
}
#[tauri::command]
pub fn rescan_applications(state: State<'_, AppState>) -> crate::scan::ScanSnapshot {
    state.scan.start(state.applications.clone())
}
#[tauri::command]
pub fn get_scan_status(state: State<'_, AppState>) -> crate::scan::ScanSnapshot {
    state.scan.snapshot()
}
#[tauri::command]
pub fn cancel_application_scan(id: u64, state: State<'_, AppState>) -> crate::scan::ScanSnapshot {
    state.scan.cancel(id)
}
#[tauri::command]
pub fn confirm_application_scan(
    id: u64,
    tokens: Vec<usize>,
    state: State<'_, AppState>,
) -> Result<crate::scan::ScanSnapshot, CommandError> {
    state
        .scan
        .confirm(id, tokens, state.applications.clone())
        .map_err(Into::into)
}
#[tauri::command]
pub async fn import_applications(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ApplicationResponse>, CommandError> {
    let records = state
        .applications
        .import_applications(paths.into_iter().map(PathBuf::from).collect())
        .await?;
    let mut out = Vec::with_capacity(records.len());
    for x in records {
        out.push(response(&state, x).await?);
    }
    Ok(out)
}
#[tauri::command]
pub async fn list_applications(
    state: State<'_, AppState>,
) -> Result<Vec<ApplicationResponse>, CommandError> {
    let records = state.applications.list_applications().await?;
    let mut out = Vec::with_capacity(records.len());
    for x in records {
        out.push(response(&state, x).await?);
    }
    Ok(out)
}
#[tauri::command]
pub async fn save_application_account(
    request: SaveApplicationAccountRequest,
    state: State<'_, AppState>,
) -> Result<ApplicationAccountResponse, CommandError> {
    state
        .applications
        .save_application_account(
            SaveApplicationAccount::new(
                request.id,
                request.application_id,
                request.display_name,
                request.username,
                request.password.map(SecretString::from),
                request.auto_submit_enabled,
            )
            .with_login_method(request.login_method, request.phone),
        )
        .await
        .map(Into::into)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn delete_application_account(
    id: ApplicationAccountId,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .applications
        .delete_application_account(id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn copy_application_username(
    id: ApplicationAccountId,
    state: State<'_, AppState>,
) -> Result<String, CommandError> {
    state
        .applications
        .copy_account_username(id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn copy_application_password(
    id: ApplicationAccountId,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .applications
        .copy_account_password(id, state.presence.clone(), state.clipboard.clone())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn launch_application(
    id: ApplicationId,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .applications
        .launch_application(id)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod login_method_wire_tests {
    use super::*;
    #[test]
    fn request_defaults_legacy_accounts_and_rejects_persisting_otp() {
        let value = serde_json::json!({"application_id": ApplicationId::new(), "display_name": "Fixture", "username": "alice", "password": "fixture"});
        let legacy: SaveApplicationAccountRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(legacy.login_method, autologin_core::LoginMethod::Password);
        let mut with_code = value;
        with_code["verification_code"] = serde_json::json!("123456");
        assert!(serde_json::from_value::<SaveApplicationAccountRequest>(with_code).is_err());
    }
}

#[cfg(test)]
mod response_tests {
    use super::validate_response_record;
    use autologin_core::{ApplicationRecord, Platform};

    #[test]
    fn application_response_rejects_an_empty_primary_target() {
        let application =
            ApplicationRecord::new(Platform::Macos, "com.example.app".into(), "Example".into())
                .unwrap();

        assert!(validate_response_record(&application).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::ApplicationAccountResponse;
    use autologin_core::{ApplicationAccount, ApplicationAccountId, ApplicationId, SecretRef};

    #[test]
    fn application_account_response_serialization_excludes_the_credential_reference() {
        let response = ApplicationAccountResponse::from(ApplicationAccount {
            id: ApplicationAccountId::new(),
            application_id: ApplicationId::new(),
            display_name: "Personal".into(),
            username: "alice".into(),
            login_method: autologin_core::LoginMethod::Password,
            phone: None,
            password_credential_ref: Some(SecretRef::new("opaque-keychain-reference").unwrap()),
            auto_submit_enabled: false,
            last_login_status: None,
            last_login_at: None,
            created_at: "1".into(),
            updated_at: "1".into(),
        });
        let value = serde_json::to_value(response).unwrap();
        assert!(value.get("password_credential_ref").is_none());
        assert!(!value.to_string().contains("opaque-keychain-reference"));
    }
}
