//! Launchable Win32 registrations, normalized independently of native sources.

use super::{shortcut::Shortcut, IconSource, WindowsAppCandidate};
use crate::WindowsLaunchTarget;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const MAX_TEXT_BYTES: usize = 8192;
const MAX_PATH_BYTES: usize = 4000;
const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
// Reserve an equal portion of the catalog raw-source budget for every registry
// view, so one large scope cannot starve the other three.
const MAX_SCOPE_ITEMS: usize = super::MAX_SOURCE_ITEMS / 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum RegistryScope {
    User64,
    User32,
    Machine64,
    Machine32,
}
impl RegistryScope {
    const ALL: [Self; 4] = [Self::User64, Self::User32, Self::Machine64, Self::Machine32];
    fn label(self) -> &'static str {
        match self {
            Self::User64 => "hkcu64",
            Self::User32 => "hkcu32",
            Self::Machine64 => "hklm64",
            Self::Machine32 => "hklm32",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RegistryText {
    pub(crate) value: String,
    pub(crate) expandable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct UninstallEntry {
    pub(crate) scope: RegistryScope,
    pub(crate) key: String,
    pub(crate) display_name: Option<RegistryText>,
    pub(crate) display_version: Option<RegistryText>,
    pub(crate) display_icon: Option<RegistryText>,
    pub(crate) install_location: Option<RegistryText>,
    pub(crate) system_component: u32,
    pub(crate) release_type: Option<RegistryText>,
    pub(crate) parent_key: Option<RegistryText>,
}

#[derive(Clone, Debug)]
pub(crate) struct SafeExecutable {
    pub(crate) path: PathBuf,
    pub(crate) fingerprint: String,
}

pub(crate) trait PathProbe {
    fn executable(&self, path: &str) -> Option<SafeExecutable>;
    fn directory(&self, path: &str) -> Option<PathBuf>;
    fn expand(&self, value: &str) -> Option<String>;
    fn scan_install(&self, path: &Path, name: &str) -> Option<SafeExecutable>;
}

fn decode_registry_text(kind: u32, bytes: &[u8]) -> Option<RegistryText> {
    if !matches!(kind, 1 | 2)
        || bytes.len() < 2
        || bytes.len() > MAX_TEXT_BYTES
        || bytes.len() % 2 != 0
    {
        return None;
    }
    let mut units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.pop()? != 0 || units.contains(&0) {
        return None;
    }
    Some(RegistryText {
        value: String::from_utf16(&units).ok()?,
        expandable: kind == 2,
    })
}

fn text_value(value: &RegistryText, probe: &impl PathProbe) -> Option<String> {
    if value.value.len() > MAX_TEXT_BYTES || value.value.contains('\0') {
        return None;
    }
    let text = if value.expandable {
        probe.expand(&value.value)?
    } else {
        value.value.clone()
    };
    (text.len() <= MAX_TEXT_BYTES && !text.contains('\0')).then_some(text)
}

pub(crate) fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Accept drive-rooted local paths only: no device namespace, ADS, UNC, traversal or commands.
pub(crate) fn local_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes.len() <= MAX_PATH_BYTES
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
        && !value[2..].contains([':', '"', '\0', '*', '?', '|', '<', '>'])
        && !value.chars().any(char::is_control)
        && value[3..]
            .split(['\\', '/'])
            .all(|part| part != "." && part != ".." && !part.ends_with([' ', '.']))
}

pub(crate) fn parse_display_icon(value: &str) -> Option<(String, i32)> {
    let value = value.trim();
    let (path, index) = if let Some(quoted) = value.strip_prefix('"') {
        let (path, rest) = quoted.split_once('"')?;
        let rest = rest.trim();
        (
            path,
            if rest.is_empty() {
                0
            } else {
                rest.strip_prefix(',')?.trim().parse().ok()?
            },
        )
    } else if let Some((path, index)) = value.rsplit_once(',') {
        if let Ok(index) = index.trim().parse::<i32>() {
            (path.trim(), index)
        } else {
            (value, 0)
        }
    } else {
        (value, 0)
    };
    let extension = Path::new(path).extension()?.to_str()?;
    (local_path(path)
        && ["exe", "dll", "ico"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate)))
    .then(|| (path.into(), index))
}

pub(crate) fn is_helper(value: &str) -> bool {
    let name = value
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    // Branding, separators and architecture/helper suffixes do not make an
    // installer/updater user-facing. A matched Start Menu shortcut is the opt-in.
    let compact: String = stem.chars().filter(char::is_ascii_alphanumeric).collect();
    let mut base = compact.as_str();
    loop {
        let trimmed = base.trim_end_matches(|ch: char| ch.is_ascii_digit());
        let trimmed = [
            "bootstrapper",
            "bootstrap",
            "helper",
            "service",
            "wizard",
            "stub",
            "arm",
            "win",
            "x",
            "v",
        ]
        .iter()
        .find_map(|suffix| trimmed.strip_suffix(suffix))
        .unwrap_or(trimmed);
        if trimmed == base {
            break;
        }
        base = trimmed;
    }
    ["unins", "uninst", "setup", "install", "update"]
        .iter()
        .any(|prefix| stem.starts_with(prefix))
        || [
            "update",
            "updater",
            "setup",
            "install",
            "installer",
            "uninstall",
            "uninstaller",
            "unins",
            "uninst",
        ]
        .iter()
        .any(|suffix| base.ends_with(suffix))
        || matches!(
            stem,
            "update32"
                | "update64"
                | "maintenancetool"
                | "repair"
                | "msiexec"
                | "rundll32"
                | "cmd"
                | "powershell"
                | "pwsh"
                | "wscript"
                | "cscript"
        )
}

pub(crate) fn is_command_host(value: &str) -> bool {
    let name = value
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "wscript.exe"
            | "cscript.exe"
            | "msiexec.exe"
            | "rundll32.exe"
            | "regsvr32.exe"
            | "mshta.exe"
    )
}

fn runtime_component_name(name: &str) -> bool {
    fn version_or_arch(token: &str) -> bool {
        matches!(
            token,
            "x64" | "x86" | "arm64" | "amd64" | "aarch64" | "bit" | "lts"
        ) || (token.chars().any(|ch| ch.is_ascii_digit())
            && token.chars().all(|ch| ch.is_ascii_digit() || ch == '.'))
    }
    fn tokens(value: &str) -> impl Iterator<Item = &str> {
        value
            .split(|ch: char| ch.is_whitespace() || matches!(ch, '-' | '(' | ')' | '[' | ']' | '_'))
            .filter(|token| !token.is_empty())
    }
    let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let family_name = normalized.strip_prefix("microsoft ").unwrap_or(&normalized);
    let known_runtime = [
        ".net runtime",
        "windows desktop runtime",
        ".net desktop runtime",
        "asp.net core runtime",
        "asp.net core shared framework",
        ".net host",
        ".net host fx resolver",
        ".net hostfxr",
        ".net apphost pack",
        ".net targeting pack",
        "edge webview2 runtime",
        "webview2 runtime",
        "java runtime environment",
        "vulkan run time libraries",
    ]
    .iter()
    .any(|prefix| {
        family_name.strip_prefix(prefix).is_some_and(|tail| {
            (tail.is_empty() || tail.starts_with([' ', '-', '(']))
                && tokens(tail).all(version_or_arch)
        })
    });
    let visual_cpp = ["visual c++", "vc++"].iter().any(|prefix| {
        family_name.strip_prefix(prefix).is_some_and(|tail| {
            tail.starts_with(' ')
                && tokens(tail).any(|token| matches!(token, "runtime" | "redistributable"))
                && tokens(tail).all(|token| {
                    version_or_arch(token)
                        || matches!(
                            token,
                            "runtime" | "redistributable" | "minimum" | "additional"
                        )
                })
        })
    });
    known_runtime || visual_cpp
}

fn component_evidence(entry: &UninstallEntry, name: &str, probe: &impl PathProbe) -> Option<bool> {
    let release = match &entry.release_type {
        Some(value) => text_value(value, probe)?,
        None => String::new(),
    };
    let parent = match &entry.parent_key {
        Some(value) => text_value(value, probe)?,
        None => String::new(),
    };
    if release.chars().any(char::is_control) || parent.chars().any(char::is_control) {
        return None;
    }
    let release = release.trim().to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    Some(
        entry.system_component == 1
            || !parent.trim().is_empty()
            || [
                "update",
                "hotfix",
                "security update",
                "update rollup",
                "service pack",
            ]
            .contains(&release.as_str())
            || ["update for ", "security update ", "hotfix ", "patch for "]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            || name.contains(" (kb")
            || runtime_component_name(&name)
            || [
                " redistributable",
                " targeting pack",
                " runtime component",
                " support component",
            ]
            .iter()
            .any(|marker| name.ends_with(marker)),
    )
}

pub(crate) fn within(path: &Path, root: &Path) -> bool {
    let path = path_key(path);
    let root = path_key(root);
    path.strip_prefix(root.trim_end_matches('\\'))
        .is_some_and(|rest| rest.starts_with('\\'))
}

pub(crate) fn narrow_install_root(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    ![
        "windows",
        "program files",
        "program files (x86)",
        "users",
        "programdata",
        "appdata",
        "local",
        "roaming",
    ]
    .contains(&name.to_ascii_lowercase().as_str())
}

/// FNV-1a 128 is only a stable non-secret identity key, never a security digest.
pub(crate) fn identity_key(source: &str, path: &Path) -> String {
    let mut hash = 0x6c62272e07bb014262b821756295c58d_u128;
    for byte in source.bytes().chain([0]).chain(path_key(path).bytes()) {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(0x1000000000000000000013b);
    }
    format!("win32:{hash:032x}")
}

pub(crate) fn normalize_uninstall_entry(
    entry: UninstallEntry,
    shortcuts: &[Shortcut],
    probe: &impl PathProbe,
) -> Option<WindowsAppCandidate> {
    let name = text_value(entry.display_name.as_ref()?, probe)?;
    let name = name.trim();
    if name.is_empty()
        || name.len() > 256
        || name.chars().any(char::is_control)
        || entry.key.is_empty()
        || entry.key.len() > 1024
        || entry.key.contains(['\0', '\\', '/'])
        || component_evidence(&entry, name, probe)?
    {
        return None;
    }
    let icon = entry
        .display_icon
        .as_ref()
        .and_then(|value| text_value(value, probe))
        .and_then(|value| parse_display_icon(&value));
    let install = entry
        .install_location
        .as_ref()
        .and_then(|value| text_value(value, probe))
        .filter(|value| local_path(value))
        .and_then(|value| probe.directory(&value))
        .filter(|path| narrow_install_root(path));
    let anchor = install.clone().or_else(|| {
        icon.as_ref()
            .and_then(|(path, _)| Path::new(path).parent())
            .and_then(|path| probe.directory(path.to_str()?))
            .filter(|path| narrow_install_root(path))
    });
    let mut selected_shortcut = None;
    let explicit = icon
        .as_ref()
        .filter(|(path, _)| !is_helper(path))
        .and_then(|(path, _)| checked_executable(probe, path))
        .filter(|executable| !is_helper(&path_key(&executable.path)));
    let executable = explicit
        .or_else(|| {
            let root = anchor.as_ref()?;
            let matching = super::shortcut::matching_shortcuts(shortcuts, name, root);
            for shortcut in matching {
                if let Some(executable) = shortcut
                    .target
                    .to_str()
                    .and_then(|path| checked_executable(probe, path))
                {
                    if within(&executable.path, root) {
                        selected_shortcut = Some(shortcut);
                        return Some(executable);
                    }
                }
            }
            None
        })
        .or_else(|| {
            install
                .as_ref()
                .and_then(|root| probe.scan_install(root, name))
                .filter(|exe| !is_helper(&path_key(&exe.path)))
        })?;
    if !local_path(executable.path.to_str()?)
        || executable.fingerprint.is_empty()
        || executable.fingerprint.len() > MAX_SIGNATURE_BYTES
    {
        return None;
    }
    let source = format!("{}:{}", entry.scope.label(), entry.key.to_lowercase());
    let identity = identity_key(&source, &executable.path);
    let mut sources = vec![source];
    if let Some(shortcut) = selected_shortcut {
        sources.push(shortcut.source.clone());
    }
    let version = entry
        .display_version
        .as_ref()
        .and_then(|value| text_value(value, probe))
        .filter(|value| {
            !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
        });
    Some(WindowsAppCandidate {
        identity,
        display_name: name.into(),
        version,
        target: WindowsLaunchTarget::Executable(executable.path),
        icon_sources: icon
            .into_iter()
            .map(|(path, index)| IconSource {
                path: PathBuf::from(path),
                index,
            })
            .collect(),
        signature_identity: executable.fingerprint,
        executable_identity: None,
        sources,
        alternate_identities: vec![],
    })
}

fn checked_executable(probe: &impl PathProbe, path: &str) -> Option<SafeExecutable> {
    if !local_path(path)
        || is_command_host(path)
        || !Path::new(path).extension()?.eq_ignore_ascii_case("exe")
    {
        return None;
    }
    let result = probe.executable(path)?;
    (local_path(result.path.to_str()?)
        && !is_command_host(&path_key(&result.path))
        && result.path.extension()?.eq_ignore_ascii_case("exe"))
    .then_some(result)
}

pub(crate) fn deduplicate(mut candidates: Vec<WindowsAppCandidate>) -> Vec<WindowsAppCandidate> {
    candidates.sort_by_key(|candidate| {
        (
            candidate.sources.clone(),
            candidate.identity.clone(),
            candidate.display_name.clone(),
            candidate.version.clone(),
        )
    });
    let mut unique: BTreeMap<String, WindowsAppCandidate> = BTreeMap::new();
    for mut candidate in candidates {
        let key = match &candidate.target {
            WindowsLaunchTarget::Executable(path) => path_key(path),
            WindowsLaunchTarget::Aumid(value) => value.clone(),
        };
        if let Some(existing) = unique.get_mut(&key) {
            existing.sources.append(&mut candidate.sources);
            existing.alternate_identities.push(candidate.identity);
            existing.icon_sources.append(&mut candidate.icon_sources);
            if existing.version.is_none() {
                existing.version = candidate.version;
            }
        } else {
            unique.insert(key, candidate);
        }
    }
    let mut result: Vec<_> = unique.into_values().collect();
    for candidate in &mut result {
        candidate.sources.sort();
        candidate.sources.dedup();
        candidate.alternate_identities.sort();
        candidate.alternate_identities.dedup();
        candidate
            .alternate_identities
            .retain(|identity| identity != &candidate.identity);
        candidate.icon_sources.sort();
        candidate.icon_sources.dedup();
    }
    result.sort_by_key(|candidate| {
        (
            candidate.display_name.to_lowercase(),
            candidate.target.encode(),
            candidate.identity.clone(),
        )
    });
    result
}

trait RegistrySource {
    fn read_scope(
        &self,
        scope: RegistryScope,
    ) -> Result<Vec<UninstallEntry>, autologin_core::AppError>;
}

fn discovery_error() -> autologin_core::AppError {
    autologin_core::AppError::new("application.discovery_unavailable")
}

fn enumerate_sources(
    registry: &impl RegistrySource,
    shortcuts: &[Shortcut],
    paths: &impl PathProbe,
) -> Result<Vec<WindowsAppCandidate>, autologin_core::AppError> {
    let mut candidates = Vec::new();
    let mut failed = false;
    for scope in RegistryScope::ALL {
        match registry.read_scope(scope) {
            Ok(entries) => candidates.extend(
                entries
                    .into_iter()
                    .take(MAX_SCOPE_ITEMS)
                    .filter_map(|entry| normalize_uninstall_entry(entry, shortcuts, paths)),
            ),
            Err(_) => failed = true,
        }
    }
    if failed {
        Err(discovery_error())
    } else {
        Ok(deduplicate(candidates))
    }
}

pub(crate) struct Win32Inventory;
impl Win32Inventory {
    pub(crate) fn enumerate(&self) -> Result<Vec<WindowsAppCandidate>, autologin_core::AppError> {
        #[cfg(windows)]
        {
            let paths = NativePathProbe;
            let shortcuts = super::shortcut::enumerate(&paths)?;
            enumerate_sources(&registry::NativeRegistry, &shortcuts, &paths)
        }
        #[cfg(not(windows))]
        {
            Err(discovery_error())
        }
    }
}

#[cfg(windows)]
pub(crate) struct NativePathProbe;
#[cfg(windows)]
impl PathProbe for NativePathProbe {
    fn executable(&self, path: &str) -> Option<SafeExecutable> {
        filesystem::executable(path)
    }
    fn directory(&self, path: &str) -> Option<PathBuf> {
        filesystem::checked_directory(path).map(|checked| checked.path.clone())
    }
    fn expand(&self, value: &str) -> Option<String> {
        filesystem::expand(value)
    }
    fn scan_install(&self, path: &Path, name: &str) -> Option<SafeExecutable> {
        filesystem::scan_install(path, name)
    }
}

#[cfg(windows)]
pub(crate) mod filesystem {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use std::{fs, os::windows::ffi::OsStrExt};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{CloseHandle, GENERIC_READ, HANDLE},
            Storage::FileSystem::{
                CreateFileW, GetDriveTypeW, GetFileInformationByHandle, GetFinalPathNameByHandleW,
                BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
                FILE_READ_ATTRIBUTES, FILE_SHARE_READ, OPEN_EXISTING,
            },
            System::Environment::ExpandEnvironmentStringsW,
        },
    };

    const MAX_SCAN_DEPTH: usize = 2;
    const MAX_SCAN_ENTRIES: usize = 256;
    const MAX_SCAN_CANDIDATES: usize = 16;
    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    pub(crate) struct CheckedPath {
        pub(crate) path: PathBuf,
        info: BY_HANDLE_FILE_INFORMATION,
        _handles: Vec<OwnedHandle>,
    }
    impl CheckedPath {
        pub(crate) fn file_size(&self) -> u64 {
            (u64::from(self.info.nFileSizeHigh) << 32) | u64::from(self.info.nFileSizeLow)
        }
        pub(crate) fn fingerprint(&self) -> Option<String> {
            fingerprint(&self.path, &self.info)
        }
    }

    fn checked_path(value: &str, directory: bool, target_access: u32) -> Option<CheckedPath> {
        if !local_path(value) {
            return None;
        }
        let drive: Vec<_> = value[..3].encode_utf16().chain([0]).collect();
        if !matches!(
            unsafe { GetDriveTypeW(PCWSTR(drive.as_ptr())) },
            2 | 3 | 5 | 6
        ) {
            return None;
        }
        let path = Path::new(value);
        let mut ancestors: Vec<_> = path.ancestors().collect();
        ancestors.reverse();
        let mut handles = Vec::new();
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        for (index, ancestor) in ancestors.iter().enumerate() {
            let wide: Vec<_> = ancestor.as_os_str().encode_wide().chain([0]).collect();
            // OPEN_REPARSE_POINT inspects each component itself. Metadata-only
            // access is sufficient for ancestor inspection, but does not enforce
            // mutation exclusion. Pinning content requires read-data access on
            // the final file plus no write/delete sharing for its retained lifetime.
            let access = if index + 1 == ancestors.len() {
                target_access
            } else {
                FILE_READ_ATTRIBUTES.0
            };
            let handle = OwnedHandle(
                unsafe {
                    CreateFileW(
                        PCWSTR(wide.as_ptr()),
                        access,
                        FILE_SHARE_READ,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                        None,
                    )
                }
                .ok()?,
            );
            unsafe { GetFileInformationByHandle(handle.0, &mut info) }.ok()?;
            let is_directory = info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
            if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                || (index + 1 < ancestors.len() && !is_directory)
                || (index + 1 == ancestors.len() && directory != is_directory)
            {
                return None;
            }
            handles.push(handle);
        }
        let handle = handles.last()?;
        let mut buffer = [0u16; 4096];
        // FILE_NAME_NORMALIZED uses the default VOLUME_NAME_DOS representation (both zero).
        let length =
            unsafe { GetFinalPathNameByHandleW(handle.0, &mut buffer, FILE_NAME_NORMALIZED) }
                as usize;
        if length == 0 || length >= buffer.len() {
            return None;
        }
        let canonical = String::from_utf16(&buffer[..length]).ok()?;
        let canonical = canonical.strip_prefix(r"\\?\").unwrap_or(&canonical);
        if !local_path(canonical) {
            return None;
        }
        Some(CheckedPath {
            path: canonical.into(),
            info,
            _handles: handles,
        })
    }

    pub(crate) fn checked_directory(value: &str) -> Option<CheckedPath> {
        checked_path(value, true, FILE_READ_ATTRIBUTES.0)
    }
    /// Retain read-data access with FILE_SHARE_READ only, preventing writes or
    /// replacement until the guard drops. Only content readers use this mode.
    pub(crate) fn checked_file(value: &str) -> Option<CheckedPath> {
        checked_path(value, false, GENERIC_READ.0)
    }

    /// Versioned metadata fingerprint, NOT an Authenticode or content-integrity assertion.
    /// Task 7 must recompute it immediately before launch and reject any mismatch.
    pub(crate) fn executable(value: &str) -> Option<SafeExecutable> {
        if is_command_host(value) || !Path::new(value).extension()?.eq_ignore_ascii_case("exe") {
            return None;
        }
        let checked = checked_path(value, false, FILE_READ_ATTRIBUTES.0)?;
        if !checked.path.extension()?.eq_ignore_ascii_case("exe")
            || is_command_host(&path_key(&checked.path))
        {
            return None;
        }
        Some(SafeExecutable {
            path: checked.path.clone(),
            fingerprint: fingerprint(&checked.path, &checked.info)?,
        })
    }

    fn fingerprint(path: &Path, info: &BY_HANDLE_FILE_INFORMATION) -> Option<String> {
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
        let modified = (u64::from(info.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(info.ftLastWriteTime.dwLowDateTime);
        let fingerprint = format!(
            "winfile-v1:{}:{:08x}:{index:016x}:{size:016x}:{modified:016x}",
            URL_SAFE_NO_PAD.encode(path_key(path)),
            info.dwVolumeSerialNumber
        );
        (fingerprint.len() <= MAX_SIGNATURE_BYTES).then_some(fingerprint)
    }

    pub(super) fn expand(value: &str) -> Option<String> {
        if value.len() > MAX_TEXT_BYTES || value.contains('\0') {
            return None;
        }
        let source: Vec<_> = value.encode_utf16().chain([0]).collect();
        let mut buffer = [0u16; 4096];
        let size = unsafe { ExpandEnvironmentStringsW(PCWSTR(source.as_ptr()), Some(&mut buffer)) }
            as usize;
        if size == 0 || size > buffer.len() || buffer[size - 1] != 0 {
            return None;
        }
        String::from_utf16(&buffer[..size - 1]).ok()
    }

    fn name_key(value: &str) -> String {
        value
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }
    pub(super) fn scan_install(path: &Path, name: &str) -> Option<SafeExecutable> {
        let root = checked_directory(path.to_str()?)?;
        if !narrow_install_root(&root.path) {
            return None;
        }
        let mut pending = vec![(root.path.clone(), 0usize)];
        let mut visited = 0usize;
        let mut candidates = Vec::new();
        while let Some((directory, depth)) = pending.pop() {
            let Some(checked) = checked_directory(directory.to_str()?) else {
                continue;
            };
            if checked.path != root.path && !within(&checked.path, &root.path) {
                return None;
            }
            let mut children = fs::read_dir(&checked.path)
                .ok()?
                .take(MAX_SCAN_ENTRIES + 1)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            children.sort_by_key(|child| path_key(&child.path()));
            for child in children {
                visited += 1;
                if visited > MAX_SCAN_ENTRIES {
                    return None;
                }
                let path = child.path();
                let Ok(kind) = child.file_type() else {
                    continue;
                };
                if kind.is_dir() && depth < MAX_SCAN_DEPTH {
                    if let Some(child) = checked_directory(path.to_str()?)
                        .filter(|child| within(&child.path, &root.path))
                    {
                        pending.push((child.path.clone(), depth + 1));
                    }
                } else if kind.is_file() && !is_helper(path.to_str()?) {
                    if let Some(candidate) = executable(path.to_str()?)
                        .filter(|candidate| within(&candidate.path, &root.path))
                    {
                        candidates.push(candidate);
                        if candidates.len() > MAX_SCAN_CANDIDATES {
                            return None;
                        }
                    }
                }
            }
        }
        candidates.sort_by_key(|candidate| path_key(&candidate.path));
        let exact: Vec<_> = candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| name_key(stem) == name_key(name))
            })
            .collect();
        if exact.len() == 1 {
            Some(exact[0].clone())
        } else if candidates.len() == 1 {
            candidates.pop()
        } else {
            None
        }
    }

    #[cfg(test)]
    mod fingerprint_tests {
        use super::*;

        #[test]
        fn fingerprint_literal_encodes_every_native_identity_component() {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            info.dwVolumeSerialNumber = 1;
            info.nFileIndexHigh = 2;
            info.nFileIndexLow = 3;
            info.nFileSizeHigh = 4;
            info.nFileSizeLow = 5;
            info.ftLastWriteTime.dwHighDateTime = 6;
            info.ftLastWriteTime.dwLowDateTime = 7;
            let expected = "winfile-v1:YzpcYS5leGU:00000001:0000000200000003:0000000400000005:0000000600000007";
            assert_eq!(
                fingerprint(Path::new(r"C:\a.exe"), &info).as_deref(),
                Some(expected)
            );
            assert_eq!(
                fingerprint(Path::new(r"c:\A.EXE"), &info).as_deref(),
                Some(expected)
            );
            assert_ne!(
                fingerprint(Path::new(r"C:\b.exe"), &info).as_deref(),
                Some(expected)
            );
            for component in 0..7 {
                let mut changed = info;
                match component {
                    0 => changed.dwVolumeSerialNumber += 1,
                    1 => changed.nFileIndexHigh += 1,
                    2 => changed.nFileIndexLow += 1,
                    3 => changed.nFileSizeHigh += 1,
                    4 => changed.nFileSizeLow += 1,
                    5 => changed.ftLastWriteTime.dwHighDateTime += 1,
                    _ => changed.ftLastWriteTime.dwLowDateTime += 1,
                }
                assert_ne!(
                    fingerprint(Path::new(r"C:\a.exe"), &changed).as_deref(),
                    Some(expected)
                );
            }
        }
    }
}

#[cfg(windows)]
mod registry {
    use super::*;
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::{
            Foundation::{
                ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND,
                ERROR_SUCCESS,
            },
            System::Registry::*,
        },
    };

    const MAX_KEY_UNITS: usize = 256;
    pub(super) struct NativeRegistry;
    struct OwnedKey(HKEY);
    impl Drop for OwnedKey {
        fn drop(&mut self) {
            let _ = unsafe { RegCloseKey(self.0) };
        }
    }

    struct Value {
        kind: REG_VALUE_TYPE,
        bytes: Vec<u8>,
    }
    fn value(key: &OwnedKey, name: &str) -> Result<Option<Value>, ()> {
        let name: Vec<_> = name.encode_utf16().chain([0]).collect();
        let mut bytes = vec![0; MAX_TEXT_BYTES];
        let mut size = bytes.len() as u32;
        let mut kind = REG_VALUE_TYPE::default();
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS || size as usize > bytes.len() {
            return Err(());
        }
        bytes.truncate(size as usize);
        Ok(Some(Value { kind, bytes }))
    }
    fn text(key: &OwnedKey, name: &str) -> Result<Option<RegistryText>, ()> {
        value(key, name)?
            .map(|value| decode_registry_text(value.kind.0, &value.bytes).ok_or(()))
            .transpose()
    }
    fn entry(key: &OwnedKey, scope: RegistryScope, name: String) -> Result<UninstallEntry, ()> {
        let system_component = match value(key, "SystemComponent")? {
            None => 0,
            Some(Value {
                kind: REG_DWORD,
                bytes,
            }) if bytes.len() == 4 => u32::from_le_bytes(bytes.try_into().map_err(|_| ())?),
            _ => return Err(()),
        };
        Ok(UninstallEntry {
            scope,
            key: name,
            system_component,
            display_name: text(key, "DisplayName")?,
            display_version: text(key, "DisplayVersion")?,
            display_icon: text(key, "DisplayIcon")?,
            install_location: text(key, "InstallLocation")?,
            release_type: text(key, "ReleaseType")?,
            parent_key: text(key, "ParentKeyName")?,
        })
    }

    fn native_scope(scope: RegistryScope) -> (HKEY, REG_SAM_FLAGS) {
        match scope {
            RegistryScope::User64 => (HKEY_CURRENT_USER, KEY_WOW64_64KEY),
            RegistryScope::User32 => (HKEY_CURRENT_USER, KEY_WOW64_32KEY),
            RegistryScope::Machine64 => (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
            RegistryScope::Machine32 => (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
        }
    }

    impl RegistrySource for NativeRegistry {
        fn read_scope(
            &self,
            scope: RegistryScope,
        ) -> Result<Vec<UninstallEntry>, autologin_core::AppError> {
            let (hive, view) = native_scope(scope);
            let mut raw = HKEY::default();
            let status = unsafe {
                RegOpenKeyExW(
                    hive,
                    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
                    None,
                    KEY_READ | view,
                    &mut raw,
                )
            };
            if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
                return Ok(vec![]);
            }
            if status != ERROR_SUCCESS {
                return Err(discovery_error());
            }
            let root = OwnedKey(raw);
            let mut entries = Vec::new();
            for index in 0..MAX_SCOPE_ITEMS as u32 {
                let mut name = [0u16; MAX_KEY_UNITS];
                let mut len = name.len() as u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        root.0,
                        index,
                        Some(PWSTR(name.as_mut_ptr())),
                        &mut len,
                        None,
                        None,
                        None,
                        None,
                    )
                };
                if status == ERROR_NO_MORE_ITEMS {
                    return Ok(entries);
                }
                if status == ERROR_MORE_DATA {
                    continue;
                }
                if status != ERROR_SUCCESS {
                    return Err(discovery_error());
                }
                if len == 0 || len as usize >= name.len() {
                    continue;
                }
                let Ok(key_name) = String::from_utf16(&name[..len as usize]) else {
                    continue;
                };
                name[len as usize] = 0;
                let mut raw = HKEY::default();
                let status = unsafe {
                    RegOpenKeyExW(
                        root.0,
                        PCWSTR(name.as_ptr()),
                        None,
                        KEY_READ | view,
                        &mut raw,
                    )
                };
                if status != ERROR_SUCCESS {
                    continue;
                }
                let key = OwnedKey(raw);
                if let Ok(entry) = entry(&key, scope, key_name) {
                    entries.push(entry);
                }
            }
            Ok(entries)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn native_scope_mapping_has_exact_hives_and_view_bits() {
            assert_eq!(
                native_scope(RegistryScope::User64),
                (HKEY_CURRENT_USER, KEY_WOW64_64KEY)
            );
            assert_eq!(
                native_scope(RegistryScope::User32),
                (HKEY_CURRENT_USER, KEY_WOW64_32KEY)
            );
            assert_eq!(
                native_scope(RegistryScope::Machine64),
                (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY)
            );
            assert_eq!(
                native_scope(RegistryScope::Machine32),
                (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::{collections::BTreeMap, path::PathBuf};

    fn fixture_text(value: &Value) -> Option<RegistryText> {
        if value.is_null() {
            return None;
        }
        Some(RegistryText {
            value: value["value"].as_str().unwrap().into(),
            expandable: value["kind"] == "expand",
        })
    }

    fn entry(value: &Value) -> UninstallEntry {
        UninstallEntry {
            scope: match value["scope"].as_str().unwrap_or("hkcu64") {
                "hklm64" => RegistryScope::Machine64,
                "hkcu32" => RegistryScope::User32,
                "hklm32" => RegistryScope::Machine32,
                _ => RegistryScope::User64,
            },
            key: value["key"].as_str().unwrap_or("app").into(),
            display_name: fixture_text(&value["name"]),
            display_version: fixture_text(&value["version"]),
            display_icon: fixture_text(&value["icon"]),
            install_location: fixture_text(&value["location"]),
            system_component: value["system"].as_u64().unwrap_or(0) as u32,
            release_type: fixture_text(&value["release"]),
            parent_key: fixture_text(&value["parent"]),
        }
    }

    #[derive(Default)]
    struct FixtureProbe {
        files: BTreeMap<String, String>,
        directories: Vec<String>,
        scans: BTreeMap<String, String>,
    }
    impl PathProbe for FixtureProbe {
        fn executable(&self, path: &str) -> Option<SafeExecutable> {
            self.files
                .get(&path.to_ascii_lowercase())
                .map(|canonical| SafeExecutable {
                    path: PathBuf::from(canonical),
                    fingerprint: "file-v1:fixture".into(),
                })
        }
        fn directory(&self, path: &str) -> Option<PathBuf> {
            self.directories
                .iter()
                .find(|value| value.eq_ignore_ascii_case(path))
                .map(PathBuf::from)
        }
        fn expand(&self, value: &str) -> Option<String> {
            if value == "%FAIL%" {
                return None;
            }
            if value == "%OVERSIZED%" {
                return Some("x".repeat(MAX_TEXT_BYTES + 1));
            }
            Some(
                value
                    .replace("%APPS%", r"C:\Apps")
                    .replace("%RELEASE%", "Update")
                    .replace("%EMPTY%", ""),
            )
        }
        fn scan_install(&self, path: &std::path::Path, _: &str) -> Option<SafeExecutable> {
            self.scans
                .get(&path.to_string_lossy().to_ascii_lowercase())
                .and_then(|target| self.executable(target))
        }
    }
    fn probe(fixture: &Value) -> FixtureProbe {
        let mut result = FixtureProbe::default();
        for path in fixture["files"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            result.files.insert(path.to_ascii_lowercase(), path.into());
        }
        if let Some(paths) = fixture["directories"].as_array() {
            result.directories = paths
                .iter()
                .map(|path| path.as_str().unwrap().into())
                .collect();
        }
        if let Some(scans) = fixture["scans"].as_object() {
            result.scans = scans
                .iter()
                .map(|(root, path)| (root.to_ascii_lowercase(), path.as_str().unwrap().into()))
                .collect();
        }
        result
    }
    fn shortcuts(fixture: &Value) -> Vec<Shortcut> {
        fixture["shortcuts"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| Shortcut {
                        name: item["name"].as_str().unwrap().into(),
                        target: PathBuf::from(item["target"].as_str().unwrap()),
                        source: item["source"]
                            .as_str()
                            .unwrap_or("user-programs:app.lnk")
                            .into(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn hand_authored_uninstall_fixtures() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/uninstall.json")).unwrap();
        for (index, fixture) in fixtures.as_array().unwrap().iter().enumerate() {
            let actual = normalize_uninstall_entry(
                entry(&fixture["entry"]),
                &shortcuts(fixture),
                &probe(fixture),
            );
            let expected = &fixture["expected"];
            if expected.is_null() {
                assert!(actual.is_none(), "fixture {index}");
            } else {
                let actual = actual.unwrap_or_else(|| panic!("missing fixture {index}"));
                assert_eq!(
                    actual.target.encode(),
                    expected["target"].as_str().unwrap(),
                    "fixture {index}"
                );
                assert_eq!(
                    actual.display_name,
                    expected["name"].as_str().unwrap(),
                    "fixture {index}"
                );
                assert_eq!(
                    actual.sources[0],
                    expected["source"].as_str().unwrap(),
                    "fixture {index}"
                );
            }
        }
    }

    #[test]
    fn branded_helper_variants_require_explicit_user_shortcuts() {
        for basename in [
            "ChatUpdate.exe",
            "ChatSetup64.exe",
            "ChatInstaller.exe",
            "chat-updater-x64.exe",
            "ChatUninstall64.exe",
            "ChatInstallHelper.exe",
        ] {
            assert!(is_helper(basename), "helper variant");
            let target = format!(r"C:\Apps\Chat\{basename}");
            let f = serde_json::json!({"entry":{"name":{"value":"Chat"},"location":{"value":"C:\\Apps\\Chat"}},
                "files":[target],"directories":["C:\\Apps\\Chat"],"shortcuts":[{"name":"Chat","target":target}]});
            let actual =
                normalize_uninstall_entry(entry(&f["entry"]), &shortcuts(&f), &probe(&f)).unwrap();
            assert_eq!(actual.sources, vec!["hkcu64:app", "user-programs:app.lnk"]);
        }
    }

    #[test]
    fn runtime_family_filters_preserve_named_user_tools() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/uninstall.json")).unwrap();
        // Literal fixture groups: eight known runtime components, four user tools.
        for (index, f) in fixtures.as_array().unwrap().iter().enumerate().skip(33) {
            let actual = normalize_uninstall_entry(entry(&f["entry"]), &[], &probe(f));
            assert_eq!(
                actual.is_none(),
                f["expected"].is_null(),
                "runtime fixture {index}"
            );
        }
    }

    #[test]
    fn release_type_registry_string_keeps_environment_syntax_literal() {
        let f = serde_json::json!({"entry":{"name":{"value":"Chat"},"release":{"value":"%RELEASE%","kind":"string"},"icon":{"value":"C:\\Apps\\Chat\\chat.exe"}},"files":["C:\\Apps\\Chat\\chat.exe"]});
        assert_eq!(
            normalize_uninstall_entry(entry(&f["entry"]), &[], &probe(&f))
                .unwrap()
                .target
                .encode(),
            r"exe:C:\Apps\Chat\chat.exe"
        );
    }

    #[test]
    fn release_type_expandable_update_is_excluded() {
        let f = serde_json::json!({"entry":{"name":{"value":"Chat"},"release":{"value":"%RELEASE%","kind":"expand"},"icon":{"value":"C:\\Apps\\Chat\\chat.exe"}},"files":["C:\\Apps\\Chat\\chat.exe"]});
        assert!(normalize_uninstall_entry(entry(&f["entry"]), &[], &probe(&f)).is_none());
    }

    #[test]
    fn component_metadata_expansion_failure_or_overflow_rejects_entry() {
        for field in ["release", "parent"] {
            for value in ["%FAIL%", "%OVERSIZED%"] {
                let mut f = serde_json::json!({"entry":{"name":{"value":"Chat"},"icon":{"value":"C:\\Apps\\Chat\\chat.exe"}},"files":["C:\\Apps\\Chat\\chat.exe"]});
                f["entry"][field] = serde_json::json!({"value":value,"kind":"expand"});
                assert!(normalize_uninstall_entry(entry(&f["entry"]), &[], &probe(&f)).is_none());
            }
        }
    }

    #[test]
    fn parent_component_metadata_obeys_registry_string_type() {
        let mut f = serde_json::json!({"entry":{"name":{"value":"Chat"},"parent":{"value":"%EMPTY%","kind":"expand"},"icon":{"value":"C:\\Apps\\Chat\\chat.exe"}},"files":["C:\\Apps\\Chat\\chat.exe"]});
        assert_eq!(
            normalize_uninstall_entry(entry(&f["entry"]), &[], &probe(&f))
                .unwrap()
                .target
                .encode(),
            r"exe:C:\Apps\Chat\chat.exe"
        );
        f["entry"]["parent"]["kind"] = "string".into();
        assert!(normalize_uninstall_entry(entry(&f["entry"]), &[], &probe(&f)).is_none());
    }

    #[test]
    fn display_icon_is_a_path_and_optional_index_never_a_command() {
        for (value, path, index) in [
            (r#""C:\Apps\Chat\chat.exe",0"#, r"C:\Apps\Chat\chat.exe", 0),
            (
                r"C:\Apps\Chat App\chat.exe,-12",
                r"C:\Apps\Chat App\chat.exe",
                -12,
            ),
            (r#""C:\Apps\a,b\chat.exe""#, r"C:\Apps\a,b\chat.exe", 0),
        ] {
            assert_eq!(parse_display_icon(value), Some((path.into(), index)));
        }
        for value in [
            r#""C:\Apps\chat.exe" --bad"#,
            r#""C:\Apps\chat.exe",x"#,
            r#""C:\Apps\chat.exe"#,
            "",
            "relative.exe",
            r"\\server\share\chat.exe",
            r"C:\Apps\chat.exe --bad",
        ] {
            assert!(parse_display_icon(value).is_none());
        }
    }

    #[test]
    fn registry_utf16_decode_is_typed_bounded_and_strict() {
        assert_eq!(decode_registry_text(1, &[65, 0, 0, 0]).unwrap().value, "A");
        assert!(!decode_registry_text(1, &[65, 0, 0, 0]).unwrap().expandable);
        assert!(decode_registry_text(2, &[65, 0, 0, 0]).unwrap().expandable);
        for (kind, bytes) in [
            (3, vec![65, 0, 0, 0]),
            (1, vec![65]),
            (1, vec![65, 0]),
            (1, vec![0, 216, 0, 0]),
            (1, vec![65, 0, 0, 0, 66, 0, 0, 0]),
            (1, vec![0; 8194]),
        ] {
            assert!(decode_registry_text(kind, &bytes).is_none());
        }
    }

    #[test]
    fn bounds_and_helpers_fail_closed() {
        let fixture = serde_json::json!({"name":{"value":"a".repeat(257)},"icon":{"value":"C:\\Apps\\chat.exe"}});
        assert!(
            normalize_uninstall_entry(entry(&fixture), &[], &FixtureProbe::default()).is_none()
        );
        for name in [
            "uninstall.exe",
            "unins000.exe",
            "updater.exe",
            "update-helper.exe",
            "Setup64.exe",
            "uninstall-helper.exe",
            "Installer.exe",
            "updateService.exe",
            "install.exe",
            "AppUpdater.exe",
        ] {
            assert!(is_helper(name));
        }
        for name in ["chat.exe", "notepad.exe", "database.exe"] {
            assert!(!is_helper(name));
        }
    }

    #[test]
    fn broad_locations_cannot_supply_shortcut_identity() {
        for location in [r"C:\", r"C:\Program Files", r"C:\Windows", r"C:\Users"] {
            let target = format!("{}\\Other\\chat.exe", location.trim_end_matches('\\'));
            let fixture = serde_json::json!({"entry":{"name":{"value":"Chat"},"location":{"value":location}},
                "files":[target],"directories":[location],"shortcuts":[{"name":"Chat","target":target}]});
            assert!(normalize_uninstall_entry(
                entry(&fixture["entry"]),
                &shortcuts(&fixture),
                &probe(&fixture)
            )
            .is_none());
        }
    }

    #[test]
    fn canonical_aliases_do_not_bypass_helper_or_command_host_filters() {
        for canonical in [r"C:\Apps\Chat\updater.exe", r"C:\Apps\Chat\powershell.exe"] {
            let fixture = serde_json::json!({"name":{"value":"Chat"},"icon":{"value":"C:\\Apps\\Chat\\alias.exe"}});
            let mut paths = FixtureProbe::default();
            paths
                .files
                .insert(r"c:\apps\chat\alias.exe".into(), canonical.into());
            assert!(normalize_uninstall_entry(entry(&fixture), &[], &paths).is_none());
        }
    }

    #[test]
    fn dedup_uses_executable_and_retains_sources_not_display_name() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/uninstall.json")).unwrap();
        let fixtures = fixture.as_array().unwrap();
        let mut candidates = Vec::new();
        for index in [0, 16, 17] {
            let f = &fixtures[index];
            candidates.push(
                normalize_uninstall_entry(entry(&f["entry"]), &shortcuts(f), &probe(f)).unwrap(),
            );
        }
        let actual = deduplicate(candidates.clone());
        candidates.reverse();
        assert_eq!(actual, deduplicate(candidates));
        assert_eq!(actual.len(), 2);
        assert_eq!(actual[0].target.encode(), r"exe:C:\Apps\Chat\chat.exe");
        assert_eq!(actual[0].sources, vec!["hkcu64:chat", "hklm64:chat"]);
        assert_eq!(actual[0].version.as_deref(), Some("2.0"));
        assert_eq!(actual[1].target.encode(), r"exe:C:\Other\chat.exe");
        assert_ne!(actual[0].identity, actual[1].identity);
    }

    #[test]
    fn source_boundary_requests_all_four_scopes_once_and_deduplicates() {
        use std::cell::RefCell;
        struct Source {
            calls: RefCell<Vec<RegistryScope>>,
        }
        impl RegistrySource for Source {
            fn read_scope(
                &self,
                scope: RegistryScope,
            ) -> Result<Vec<UninstallEntry>, autologin_core::AppError> {
                self.calls.borrow_mut().push(scope);
                let fixtures: Value =
                    serde_json::from_str(include_str!("../../tests/fixtures/uninstall.json"))
                        .unwrap();
                Ok(match scope {
                    RegistryScope::User64 => vec![entry(&fixtures[0]["entry"])],
                    RegistryScope::Machine64 => vec![entry(&fixtures[16]["entry"])],
                    RegistryScope::Machine32 => vec![entry(&fixtures[17]["entry"])],
                    RegistryScope::User32 => vec![], // A missing root is an empty source.
                })
            }
        }
        let registry = Source {
            calls: RefCell::new(vec![]),
        };
        let mut paths = FixtureProbe::default();
        paths.files.insert(
            r"c:\apps\chat\chat.exe".into(),
            r"C:\Apps\Chat\chat.exe".into(),
        );
        paths
            .files
            .insert(r"c:\other\chat.exe".into(), r"C:\Other\chat.exe".into());
        let result = enumerate_sources(&registry, &[], &paths).unwrap();
        assert_eq!(
            *registry.calls.borrow(),
            vec![
                RegistryScope::User64,
                RegistryScope::User32,
                RegistryScope::Machine64,
                RegistryScope::Machine32
            ]
        );
        assert_eq!(
            result
                .iter()
                .map(|item| item.target.encode())
                .collect::<Vec<_>>(),
            vec![r"exe:C:\Apps\Chat\chat.exe", r"exe:C:\Other\chat.exe"]
        );
        assert_eq!(result[0].sources, vec!["hkcu64:chat", "hklm64:chat"]);
    }

    #[test]
    fn raw_registry_budget_counts_malformed_records_before_normalization() {
        struct Source;
        impl RegistrySource for Source {
            fn read_scope(
                &self,
                scope: RegistryScope,
            ) -> Result<Vec<UninstallEntry>, autologin_core::AppError> {
                let f = serde_json::json!({"name":{"value":"Chat"},"icon":{"value":"C:\\Apps\\Chat\\chat.exe"}});
                let mut bad = entry(&f);
                bad.scope = scope;
                bad.display_name = None;
                let mut items = vec![bad; 1024];
                items.push(entry(&f));
                Ok(items)
            }
        }
        let f = serde_json::json!({"files":["C:\\Apps\\Chat\\chat.exe"]});
        assert!(enumerate_sources(&Source, &[], &probe(&f))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn source_failure_is_sanitized_and_other_scopes_are_still_requested() {
        use std::cell::Cell;
        struct Source(Cell<usize>);
        impl RegistrySource for Source {
            fn read_scope(
                &self,
                scope: RegistryScope,
            ) -> Result<Vec<UninstallEntry>, autologin_core::AppError> {
                self.0.set(self.0.get() + 1);
                if scope == RegistryScope::User64 {
                    Err(autologin_core::AppError::new("private_source_detail"))
                } else {
                    Ok(vec![])
                }
            }
        }
        let source = Source(Cell::new(0));
        let error = enumerate_sources(&source, &[], &FixtureProbe::default()).unwrap_err();
        assert_eq!(source.0.get(), 4);
        assert_eq!(error.code(), "application.discovery_unavailable");
        assert!(error.params.is_empty());
    }

    #[test]
    fn explicit_user_facing_shortcut_can_select_helper_but_not_command_host() {
        let f = serde_json::json!({"entry":{"name":{"value":"Chat"},"location":{"value":"C:\\Apps\\Chat"}},
            "files":["C:\\Apps\\Chat\\updater.exe"],"directories":["C:\\Apps\\Chat"],
            "shortcuts":[{"name":"Chat","target":"C:\\Apps\\Chat\\updater.exe"}]});
        let actual =
            normalize_uninstall_entry(entry(&f["entry"]), &shortcuts(&f), &probe(&f)).unwrap();
        assert_eq!(actual.target.encode(), r"exe:C:\Apps\Chat\updater.exe");
        assert_eq!(actual.sources, vec!["hkcu64:app", "user-programs:app.lnk"]);
        let mut f = f;
        f["files"][0] = r"C:\Apps\Chat\cmd.exe".into();
        f["shortcuts"][0]["target"] = r"C:\Apps\Chat\cmd.exe".into();
        assert!(
            normalize_uninstall_entry(entry(&f["entry"]), &shortcuts(&f), &probe(&f)).is_none()
        );
    }

    #[cfg(windows)]
    mod filesystem {
        use super::*;
        use std::fs;

        struct Tree(PathBuf);
        impl Tree {
            fn new() -> Self {
                let path = std::env::temp_dir()
                    .join(format!("logindeck-inventory-test-{}", uuid::Uuid::new_v4()));
                fs::create_dir(&path).unwrap();
                Self(path)
            }
            fn file(&self, relative: &str) -> PathBuf {
                let path = self.0.join(relative);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, b"fixture").unwrap();
                path
            }
        }
        impl Drop for Tree {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn native_path_requires_existing_local_executable_and_repeatable_fingerprint() {
            let tree = Tree::new();
            let path = tree.file("chat.exe");
            let probe = NativePathProbe;
            let first = probe.executable(path.to_str().unwrap()).unwrap();
            assert_eq!(first.path, path);
            assert_eq!(
                first.fingerprint,
                probe
                    .executable(path.to_str().unwrap())
                    .unwrap()
                    .fingerprint
            );
            assert!(first.fingerprint.starts_with("winfile-v1:"));
            assert!(first.fingerprint.len() < 16 * 1024);
            fs::write(&path, b"different-size").unwrap();
            assert_ne!(
                first.fingerprint,
                probe
                    .executable(path.to_str().unwrap())
                    .unwrap()
                    .fingerprint
            );
            for path in [
                "relative.exe".into(),
                PathBuf::from(r"\\server\share\chat.exe"),
                tree.0.join("missing.exe"),
                tree.file("chat.dll"),
                tree.0.clone(),
            ] {
                assert!(probe.executable(path.to_str().unwrap()).is_none());
            }
        }

        #[test]
        fn install_search_is_bounded_and_rejects_ambiguous_candidates() {
            let tree = Tree::new();
            let chat = tree.file("bin/chat.exe");
            tree.file("unins000.exe");
            let probe = NativePathProbe;
            assert_eq!(probe.scan_install(&tree.0, "Chat").unwrap().path, chat);
            tree.file("other.exe");
            assert!(probe.scan_install(&tree.0, "Unknown").is_none());
            assert_eq!(probe.scan_install(&tree.0, "Chat").unwrap().path, chat);
            assert!(probe.scan_install(Path::new(r"C:\"), "Chat").is_none());
            let deep = Tree::new();
            deep.file("a/b/c/deep.exe");
            assert!(probe.scan_install(&deep.0, "Deep").is_none());
            let many = Tree::new();
            for index in 0..17 {
                many.file(&format!("candidate{index}.exe"));
            }
            assert!(probe.scan_install(&many.0, "candidate0").is_none());
            let entries = Tree::new();
            entries.file("chat.exe");
            for index in 0..256 {
                entries.file(&format!("data{index}.txt"));
            }
            assert!(probe.scan_install(&entries.0, "Chat").is_none());
        }

        #[test]
        fn sole_branded_helper_in_install_location_is_not_an_app() {
            let tree = Tree::new();
            for basename in [
                "ChatUpdate.exe",
                "ChatSetup64.exe",
                "ChatInstaller.exe",
                "ChatUninstall64.exe",
                "ChatInstallHelper.exe",
            ] {
                let path = tree.file(basename);
                assert!(NativePathProbe.scan_install(&tree.0, "Chat").is_none());
                fs::remove_file(path).unwrap();
            }
        }

        fn junction(path: &Path, target: &Path) {
            use std::os::windows::ffi::OsStrExt;
            use windows::{
                core::PCWSTR,
                Win32::{
                    Foundation::{CloseHandle, GENERIC_WRITE},
                    Storage::FileSystem::{
                        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
                    },
                    System::IO::DeviceIoControl,
                },
            };
            fs::create_dir(path).unwrap();
            let substitute = format!(r"\??\{}", target.display());
            let units: Vec<u16> = substitute.encode_utf16().chain([0]).collect();
            let string_bytes = (units.len() * 2) as u16;
            let mut buffer = Vec::new();
            buffer.extend_from_slice(&0xA0000003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
            buffer.extend_from_slice(&(8 + string_bytes + 2).to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            buffer.extend_from_slice(&(string_bytes - 2).to_le_bytes());
            buffer.extend_from_slice(&string_bytes.to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            for unit in units {
                buffer.extend_from_slice(&unit.to_le_bytes());
            }
            buffer.extend_from_slice(&0u16.to_le_bytes());
            let wide: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
            let handle = unsafe {
                CreateFileW(
                    PCWSTR(wide.as_ptr()),
                    GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    None,
                )
            }
            .unwrap();
            let mut returned = 0;
            let result = unsafe {
                DeviceIoControl(
                    handle,
                    0x000900A4,
                    Some(buffer.as_ptr().cast()),
                    buffer.len() as u32,
                    None,
                    0,
                    Some(&mut returned),
                    None,
                )
            };
            unsafe { CloseHandle(handle) }.unwrap();
            result.unwrap();
        }

        #[test]
        fn native_reparse_junction_is_rejected_as_root_ancestor_and_scan_child() {
            let tree = Tree::new();
            let outside = Tree::new();
            outside.file("chat.exe");
            let link = tree.0.join("junction");
            junction(&link, &outside.0);
            let probe = NativePathProbe;
            assert!(probe.directory(link.to_str().unwrap()).is_none());
            assert!(probe
                .executable(link.join("chat.exe").to_str().unwrap())
                .is_none());
            assert!(probe.scan_install(&link, "Chat").is_none());
            assert!(probe.scan_install(&tree.0, "Chat").is_none());
            fs::remove_dir(&link).unwrap();
            assert!(outside.0.join("chat.exe").exists());
        }
    }
}
