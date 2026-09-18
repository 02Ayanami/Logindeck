use std::path::PathBuf;

use autologin_core::AppError;

const MAX_ENCODED_TARGET_LENGTH: usize = 4_096;
const TARGET_FIELD: &str = "launch_target";

/// A launch target whose scheme has already been validated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowsLaunchTarget {
    Executable(PathBuf),
    Aumid(String),
}

impl WindowsLaunchTarget {
    /// Parses the persisted, explicitly typed Windows launch-target encoding.
    pub fn parse(value: &str) -> Result<Self, AppError> {
        if value.chars().count() > MAX_ENCODED_TARGET_LENGTH || value.contains('\0') {
            return Err(invalid_target());
        }

        if let Some(path) = value.strip_prefix("exe:") {
            let path = PathBuf::from(path);
            if !path.is_absolute() || !has_exe_extension(&path) {
                return Err(invalid_target());
            }
            return Ok(Self::Executable(path));
        }

        if let Some(aumid) = value.strip_prefix("aumid:") {
            if aumid.trim().is_empty() {
                return Err(invalid_target());
            }
            return Ok(Self::Aumid(aumid.to_owned()));
        }

        Err(invalid_target())
    }

    /// Returns the persisted explicit scheme without shell-command interpretation.
    pub fn encode(&self) -> String {
        match self {
            Self::Executable(path) => format!("exe:{}", path.display()),
            Self::Aumid(aumid) => format!("aumid:{aumid}"),
        }
    }
}

fn has_exe_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

fn invalid_target() -> AppError {
    AppError::invalid_field(TARGET_FIELD)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::WindowsLaunchTarget;

    #[test]
    fn target_round_trips_without_guessing() {
        let executable = WindowsLaunchTarget::Executable(PathBuf::from(r"C:\Apps\Chat\chat.exe"));
        let aumid = WindowsLaunchTarget::Aumid("Contoso.Chat_abc!App".into());

        assert_eq!(
            WindowsLaunchTarget::parse(&executable.encode()).unwrap(),
            executable
        );
        assert_eq!(WindowsLaunchTarget::parse(&aumid.encode()).unwrap(), aumid);
    }

    #[test]
    fn target_rejects_every_invalid_encoding_class() {
        let executable_with_nul = "exe:C:\\Apps\\\0Chat\\chat.exe";
        let aumid_with_nul = "aumid:Contoso\0Chat!App";
        let overlong = format!("aumid:{}", "a".repeat(4_091));
        for value in [
            "exe:chat.exe",
            "exe:C:\\Apps\\Chat\\chat.cmd",
            executable_with_nul,
            "aumid:",
            "aumid:   ",
            aumid_with_nul,
            "powershell:calc",
            &overlong,
        ] {
            let error = WindowsLaunchTarget::parse(value).unwrap_err();
            assert_eq!(error.code(), "validation.invalid_field");
            assert_eq!(error.params.get("field"), Some(&"launch_target".to_owned()));
        }
    }
}
