use crate::{commands::CommandError, state::AppState};
use autologin_core::Locale;
use serde::Serialize;
use tauri::State;
#[derive(Serialize)]
pub struct SettingsResponse {
    pub ui_locale: Locale,
}
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<SettingsResponse, CommandError> {
    Ok(SettingsResponse {
        ui_locale: state.settings.locale().await?,
    })
}
#[tauri::command]
pub async fn set_locale(
    locale: Locale,
    state: State<'_, AppState>,
) -> Result<SettingsResponse, CommandError> {
    state.settings.set_locale(locale).await?;
    Ok(SettingsResponse { ui_locale: locale })
}

#[derive(Serialize)]
pub struct CredentialMaintenance {
    pub pending_count: usize,
    pub storage_backend: &'static str,
    pub last_os_status: Option<i32>,
    pub last_error: Option<CommandError>,
}
fn credential_backend() -> (&'static str, Option<i32>) {
    (
        platform_runtime::CREDENTIAL_BACKEND,
        platform_runtime::credential_last_status(),
    )
}
#[tauri::command]
pub async fn get_credential_maintenance(
    state: State<'_, AppState>,
) -> Result<CredentialMaintenance, CommandError> {
    let (storage_backend, last_os_status) = credential_backend();
    Ok(CredentialMaintenance {
        pending_count: state.vault.pending_cleanup_count().await?,
        storage_backend,
        last_os_status,
        last_error: None,
    })
}
#[tauri::command]
pub async fn retry_credential_cleanup(
    state: State<'_, AppState>,
) -> Result<CredentialMaintenance, CommandError> {
    let last_error = state
        .vault
        .retry_pending_secret_cleanup()
        .await
        .err()
        .map(Into::into);
    let (storage_backend, last_os_status) = credential_backend();
    Ok(CredentialMaintenance {
        pending_count: state.vault.pending_cleanup_count().await?,
        storage_backend,
        last_os_status,
        last_error,
    })
}

#[tauri::command]
pub async fn get_browser_capture_settings(
    state: State<'_, AppState>,
) -> Result<autologin_core::BrowserCaptureSettings, CommandError> {
    Ok(state.settings.browser_capture().await?)
}
#[tauri::command]
pub async fn set_browser_capture_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<autologin_core::BrowserCaptureSettings, CommandError> {
    Ok(state.settings.set_browser_capture(enabled).await?)
}
