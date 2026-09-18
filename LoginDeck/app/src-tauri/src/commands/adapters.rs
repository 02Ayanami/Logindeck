use crate::{adapters::AdapterPreview, state::AppState};
use autologin_core::ApplicationId;
use tauri::State;
#[tauri::command]
pub async fn inspect_application_adapter(
    id: ApplicationId,
    path: String,
    state: State<'_, AppState>,
) -> Result<AdapterPreview, String> {
    let app = state
        .applications
        .get_application(id)
        .await
        .map_err(|_| "app_missing")?;
    if path.len() > 4096 || !path.ends_with(".json") {
        return Err("invalid_adapter".into());
    }
    state.adapters.inspect(&app, std::path::Path::new(&path))
}
#[tauri::command]
pub async fn install_application_adapter(
    id: ApplicationId,
    source: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app = state
        .applications
        .get_application(id)
        .await
        .map_err(|_| "app_missing")?;
    state.adapters.install(&app, &source)
}
#[tauri::command]
pub async fn remove_application_adapter(
    id: ApplicationId,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app = state
        .applications
        .get_application(id)
        .await
        .map_err(|_| "app_missing")?;
    state.adapters.remove(&app)
}
