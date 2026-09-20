use autologin_core::AppError;
use std::path::PathBuf;

const APPLICATION_IDENTIFIER: &str = "com.autologin.desktop";

#[cfg(target_os = "windows")]
pub fn application_data_directory() -> Result<PathBuf, AppError> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        FOLDERID_RoamingAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };

    let known_folder = unsafe {
        SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None)
            .map_err(|_| AppError::new("platform.data_directory_unavailable"))?
    };
    let result = unsafe { known_folder.to_string() }
        .map(PathBuf::from)
        .map(|path| path.join(APPLICATION_IDENTIFIER))
        .map_err(|_| AppError::new("platform.data_directory_unavailable"));
    unsafe { CoTaskMemFree(Some(known_folder.0.cast())) };
    result
}

#[cfg(not(target_os = "windows"))]
pub fn application_data_directory() -> Result<PathBuf, AppError> {
    Err(AppError::new("platform.unsupported"))
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    #[test]
    fn resolves_roaming_app_data_for_the_fixed_application_identifier() {
        let path = super::application_data_directory().expect("roaming application data");
        assert!(path.is_absolute());
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(super::APPLICATION_IDENTIFIER)
        );
    }
}
