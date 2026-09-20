use autologin_core::AppError;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
const APPLICATION_IDENTIFIER: &str = "com.autologin.desktop";

#[cfg(target_os = "macos")]
pub fn application_data_directory() -> Result<PathBuf, AppError> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::new("platform.data_directory_unavailable"))?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(AppError::new("platform.data_directory_unavailable"));
    }

    Ok(home
        .join("Library")
        .join("Application Support")
        .join(APPLICATION_IDENTIFIER))
}

#[cfg(not(target_os = "macos"))]
pub fn application_data_directory() -> Result<PathBuf, AppError> {
    Err(AppError::new("platform.unsupported"))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn resolves_application_support_for_the_fixed_application_identifier() {
        let path = super::application_data_directory().expect("application support directory");
        assert!(path.is_absolute());
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(super::APPLICATION_IDENTIFIER)
        );
    }
}
