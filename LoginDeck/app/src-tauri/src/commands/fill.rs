use crate::{fill::FillSnapshot, state::AppState};
use autologin_core::ApplicationAccountId;
use tauri::{Manager, State};
#[tauri::command]
pub fn start_application_fill(
    account_id: ApplicationAccountId,
    handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<FillSnapshot, String> {
    state
        .fill
        .start(account_id, state.applications.clone(), move || {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.set_focus();
            }
        })
}
#[tauri::command]
pub fn get_application_fill(id: u64, state: State<'_, AppState>) -> Result<FillSnapshot, String> {
    state.fill.status(id)
}
#[tauri::command]
pub fn continue_application_fill(id: u64, state: State<'_, AppState>) -> Result<(), String> {
    state.fill.proceed(id)
}
#[tauri::command]
pub fn cancel_application_fill(id: u64, state: State<'_, AppState>) {
    state.fill.cancel(id);
}
#[tauri::command]
pub fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map(|_| ())
            .map_err(|_| "unavailable".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("unsupported".into())
    }
}

#[tauri::command]
pub async fn start_application_switch(
    account_id: ApplicationAccountId,
    handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<FillSnapshot, String> {
    let account = state
        .applications
        .get_account(account_id)
        .await
        .map_err(|_| "account_changed")?;
    let app = state
        .applications
        .get_application(account.application_id)
        .await
        .map_err(|_| "app_missing")?;
    let definition = state.adapters.load(&app)?.ok_or("adapter_missing")?;
    let plan = crate::adapters::SwitchPlan::new(definition)?;
    state.fill.start_with_plan(
        account_id,
        state.applications.clone(),
        Some(plan),
        move || {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.set_focus();
            }
        },
    )
}
