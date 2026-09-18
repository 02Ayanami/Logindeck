use crate::{commands::CommandError, edge_install};
use autologin_core::AppError;
use tauri::Manager;
static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[tauri::command]
pub async fn install_edge_extension(
    app: tauri::AppHandle,
) -> Result<edge_install::Installation, CommandError> {
    if !cfg!(target_os = "macos") {
        return Err(AppError::new("platform.unsupported").into());
    }
    let _guard = INSTALL_LOCK.lock().await;
    let installation_root = installation_root(&app)?;
    let mut resource = app
        .path()
        .resource_dir()
        .map_err(|_| AppError::new("extension.resources_missing"))?
        .join("edge");
    // Development runs outside an app bundle; shipped apps must use their own resources.
    if cfg!(debug_assertions)
        && !resource.is_dir()
        && !resource
            .ancestors()
            .any(|path| path.extension().is_some_and(|ext| ext == "app"))
    {
        resource = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/edge");
    }
    tauri::async_runtime::spawn_blocking(move || {
        edge_install::install(&installation_root, &resource)
    })
    .await
    .map_err(|_| AppError::new("extension.setup_failed"))?
    .map_err(Into::into)
}
#[tauri::command]
pub async fn open_edge_extensions() -> Result<(), CommandError> {
    let result = tauri::async_runtime::spawn_blocking(|| open_edge_extensions_native()).await;
    match result {
        Ok(Ok(status)) if status.success() => Ok(()),
        _ => Err(AppError::new("extension.edge_unavailable").into()),
    }
}
#[tauri::command]
pub async fn open_edge_extension_folder(app: tauri::AppHandle) -> Result<(), CommandError> {
    let path = edge_install::extension_path(&installation_root(&app)?);
    if !path.join("manifest.json").is_file() {
        return Err(AppError::new("extension.resources_missing").into());
    }
    let result = tauri::async_runtime::spawn_blocking(move || reveal_native(path)).await;
    match result {
        Ok(Ok(status)) if status.success() => Ok(()),
        _ => Err(AppError::new("extension.setup_failed").into()),
    }
}

fn installation_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, CommandError> {
    app.path()
        .home_dir()
        .map_err(|_| AppError::new("extension.setup_failed").into())
}

fn open_edge_extensions_native() -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new("/usr/bin/open")
        .args(["-a", "Microsoft Edge", "edge://extensions/"])
        .status()
}

fn reveal_native(path: std::path::PathBuf) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .status()
}
