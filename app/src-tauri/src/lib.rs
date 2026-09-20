mod commands;
mod edge_install;
mod scan;
mod state;

use commands::{applications::*, edge::*, settings::*, websites::*};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = tauri::async_runtime::block_on(state::AppState::initialize(app.handle()))
                .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_websites,
            next_account_number,
            edit_website_group,
            remove_application,
            save_website,
            delete_website,
            copy_website_username,
            copy_website_password,
            rescan_applications,
            get_scan_status,
            cancel_application_scan,
            confirm_application_scan,
            import_applications,
            list_applications,
            get_application_icon,
            get_scan_candidate_icon,
            save_application_account,
            delete_application_account,
            copy_application_username,
            copy_application_password,
            launch_application,
            install_edge_extension,
            open_edge_extensions,
            open_edge_extension_folder,
            get_settings,
            get_browser_capture_settings,
            set_browser_capture_enabled,
            set_locale,
            get_credential_maintenance,
            retry_credential_cleanup,
        ])
        .build(tauri::generate_context!())
        .expect("error while building LoginDeck")
        .run(|handle, event| {
            if let tauri::RunEvent::Exit = event {
                let state = handle.state::<state::AppState>();
                let result = tauri::async_runtime::block_on(async {
                    for _ in 0..2 {
                        if state.clipboard.shutdown().await.is_ok() {
                            return Ok(());
                        }
                    }
                    state.clipboard.shutdown().await
                });
                if result.is_err() {
                    eprintln!("Password clipboard cleanup failed during exit");
                }
            }
        });
}
