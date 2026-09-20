use crate::{commands::CommandError, edge_install};
use autologin_core::AppError;
use tauri::Manager;

static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub async fn install_edge_extension(
    app: tauri::AppHandle,
) -> Result<edge_install::Installation, CommandError> {
    let _guard = INSTALL_LOCK.lock().await;
    let resource = edge_resource(&app)?;

    #[cfg(target_os = "windows")]
    let task = tauri::async_runtime::spawn_blocking(move || {
        platform_runtime::edge::install(&resource).map(|path| edge_install::Installation {
            extension_path: path.to_string_lossy().into_owned(),
        })
    });

    #[cfg(target_os = "macos")]
    let task = {
        let installation_root = installation_root(&app)?;
        tauri::async_runtime::spawn_blocking(move || {
            edge_install::install(&installation_root, &resource)
        })
    };

    task.await
        .map_err(|_| AppError::new("extension.setup_failed"))?
        .map_err(Into::into)
}

fn edge_resource(app: &tauri::AppHandle) -> Result<std::path::PathBuf, CommandError> {
    let mut resource = app
        .path()
        .resource_dir()
        .map_err(|_| AppError::new("extension.resources_missing"))?
        .join("edge");
    if cfg!(debug_assertions)
        && !resource.is_dir()
        && !resource
            .ancestors()
            .any(|path| path.extension().is_some_and(|ext| ext == "app"))
    {
        resource = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/edge");
    }
    Ok(resource)
}

#[tauri::command]
pub async fn open_edge_extensions() -> Result<(), CommandError> {
    #[cfg(target_os = "windows")]
    let result =
        tauri::async_runtime::spawn_blocking(platform_runtime::edge::open_extensions).await;

    #[cfg(target_os = "macos")]
    let result = tauri::async_runtime::spawn_blocking(open_edge_extensions_native).await;

    match result {
        Ok(Ok(())) => Ok(()),
        _ => Err(AppError::new("extension.edge_unavailable").into()),
    }
}

#[tauri::command]
pub async fn open_edge_extension_folder(app: tauri::AppHandle) -> Result<(), CommandError> {
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        let path = platform_runtime::edge::extension_path()?;
        if !path.join("manifest.json").is_file() {
            return Err(AppError::new("extension.resources_missing").into());
        }
        tauri::async_runtime::spawn_blocking(platform_runtime::edge::reveal_extension)
            .await
            .map_err(|_| AppError::new("extension.setup_failed"))?
            .map_err(Into::into)
    }

    #[cfg(target_os = "macos")]
    {
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
}

#[cfg(target_os = "macos")]
fn installation_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, CommandError> {
    app.path()
        .home_dir()
        .map_err(|_| AppError::new("extension.setup_failed").into())
}

#[cfg(target_os = "macos")]
fn open_edge_extensions_native() -> std::io::Result<()> {
    let status = std::process::Command::new("/usr/bin/open")
        .args(["-a", "Microsoft Edge", "edge://extensions/"])
        .status()?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| std::io::Error::other("Edge unavailable"))
}

#[cfg(target_os = "macos")]
fn reveal_native(path: std::path::PathBuf) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .status()
}
