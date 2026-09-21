//! Safe Start Menu shortcut index.

use super::win32::{is_command_host, local_path, path_key, within};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Shortcut {
    pub(crate) name: String,
    pub(crate) target: PathBuf,
    pub(crate) source: String,
}

fn decode_buffer(buffer: &[u16]) -> Option<String> {
    let end = buffer.iter().position(|unit| *unit == 0)?;
    // A full buffer may be a successful-but-truncated shell API result.
    if end == buffer.len().checked_sub(1)? {
        return None;
    }
    String::from_utf16(&buffer[..end]).ok()
}

fn parse_shortcut(name: &str, target: &str, arguments: &str, source: &str) -> Option<Shortcut> {
    if name.trim().is_empty()
        || name.len() > 256
        || name.chars().any(char::is_control)
        || !arguments.is_empty()
        || !local_path(target)
        || is_command_host(target)
        || !Path::new(target).extension()?.eq_ignore_ascii_case("exe")
        || source.is_empty()
        || source.len() > 4096
        || source.contains('\0')
    {
        return None;
    }
    Some(Shortcut {
        name: name.trim().into(),
        target: target.into(),
        source: source.into(),
    })
}

pub(crate) fn matching_shortcuts<'a>(
    shortcuts: &'a [Shortcut],
    name: &str,
    root: &Path,
) -> Vec<&'a Shortcut> {
    let mut matched: Vec<_> = shortcuts
        .iter()
        .filter(|shortcut| {
            within(&shortcut.target, root) && shortcut.name.eq_ignore_ascii_case(name)
        })
        .collect();
    matched.sort_by_key(|shortcut| (path_key(&shortcut.target), &shortcut.source));
    matched
}

#[cfg(windows)]
pub(crate) fn enumerate(
    probe: &impl super::win32::PathProbe,
) -> Result<Vec<Shortcut>, autologin_core::AppError> {
    native::enumerate(probe)
}

#[cfg(windows)]
mod native {
    use super::super::win32::PathProbe;
    use super::*;
    use std::{
        fs,
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
    };
    use windows::{
        core::{Interface, PCWSTR},
        Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ, STGM_SHARE_DENY_WRITE,
            },
            UI::Shell::{
                FOLDERID_CommonPrograms, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath,
                ShellLink, KF_FLAG_DONT_VERIFY, SLGP_RAWPATH,
            },
        },
    };

    // Two Start Menu roots share one bounded auxiliary inventory budget.
    const MAX_ENTRIES: usize = super::super::MAX_SOURCE_ITEMS / 2;
    const MAX_DEPTH: usize = 6;
    const MAX_LINK_BYTES: u64 = 1024 * 1024;

    struct Apartment(bool);
    impl Apartment {
        fn enter() -> Option<Self> {
            let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            if status.is_ok() {
                Some(Self(true))
            } else if status == RPC_E_CHANGED_MODE {
                Some(Self(false))
            } else {
                None
            }
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    struct FolderMemory(windows::core::PWSTR);
    impl Drop for FolderMemory {
        fn drop(&mut self) {
            unsafe { CoTaskMemFree(Some(self.0 .0.cast())) };
        }
    }

    fn known_folder(id: &windows::core::GUID) -> Option<String> {
        let memory =
            FolderMemory(unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY, None) }.ok()?);
        if memory.0.is_null() {
            return None;
        }
        // The known-folder API owns a NUL-terminated allocation; cap copying.
        let mut units = Vec::new();
        for index in 0..4096 {
            let unit = unsafe { *memory.0 .0.add(index) };
            if unit == 0 {
                return String::from_utf16(&units).ok();
            }
            units.push(unit);
        }
        None
    }

    fn pin_link(
        path: &Path,
        probe: &impl PathProbe,
    ) -> Option<super::super::win32::filesystem::CheckedPath> {
        if !local_path(path.to_str()?) || !path.extension()?.eq_ignore_ascii_case("lnk") {
            return None;
        }
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.is_file()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || metadata.len() > MAX_LINK_BYTES
        {
            return None;
        }
        probe.directory(path.parent()?.to_str()?)?;
        // Pin link contents with read-data access and no write/delete sharing.
        // Retain the guard across both the size check and the COM read.
        let checked = super::super::win32::filesystem::checked_file(path.to_str()?)?;
        // The initial observation is only a fast rejection. The authoritative
        // length comes from the same retained handle that pins the file for COM.
        (checked.file_size() <= MAX_LINK_BYTES).then_some(checked)
    }

    fn load(path: &Path, source: &str, probe: &impl PathProbe) -> Option<Shortcut> {
        let _checked = pin_link(path, probe)?;
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let persisted: IPersistFile = link.cast().ok()?;
        let file_name: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        unsafe {
            persisted.Load(
                PCWSTR(file_name.as_ptr()),
                STGM_READ | STGM_SHARE_DENY_WRITE,
            )
        }
        .ok()?;
        let mut target = [0u16; 4096];
        let mut arguments = [0u16; 4096];
        // Do not call Resolve: it can search, repair, contact remote paths, or show UI.
        unsafe { link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }.ok()?;
        unsafe { link.GetArguments(&mut arguments) }.ok()?;
        let mut shortcut = parse_shortcut(
            path.file_stem()?.to_str()?,
            &decode_buffer(&target)?,
            &decode_buffer(&arguments)?,
            source,
        )?;
        shortcut.target = probe.executable(shortcut.target.to_str()?)?.path;
        Some(shortcut)
    }

    pub(super) fn enumerate(
        probe: &impl PathProbe,
    ) -> Result<Vec<Shortcut>, autologin_core::AppError> {
        let _apartment = Apartment::enter()
            .ok_or_else(|| autologin_core::AppError::new("application.discovery_unavailable"))?;
        let mut shortcuts = Vec::new();
        for (id, label) in [
            (FOLDERID_Programs, "user-programs"),
            (FOLDERID_CommonPrograms, "common-programs"),
        ] {
            let Some(root) = known_folder(&id).and_then(|path| probe.directory(&path)) else {
                continue;
            };
            let Some(_root_guard) = root
                .to_str()
                .and_then(super::super::win32::filesystem::checked_directory)
            else {
                continue;
            };
            let mut pending = vec![(root.clone(), 0usize)];
            let mut visited = 0;
            let mut exceeded = false;
            let start = shortcuts.len();
            while let Some((directory, depth)) = pending.pop() {
                if probe.directory(directory.to_str().unwrap_or("")).is_none() {
                    continue;
                }
                let Some(_directory_guard) = directory
                    .to_str()
                    .and_then(super::super::win32::filesystem::checked_directory)
                else {
                    continue;
                };
                let Ok(children) = fs::read_dir(&directory) else {
                    continue;
                };
                let mut children = children
                    .take(MAX_ENTRIES + 1)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap_or_default();
                children.sort_by_key(|child| path_key(&child.path()));
                for child in children {
                    visited += 1;
                    if visited > MAX_ENTRIES {
                        exceeded = true;
                        break;
                    }
                    let path = child.path();
                    let Ok(metadata) = fs::symlink_metadata(&path) else {
                        continue;
                    };
                    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                        continue;
                    }
                    if metadata.is_dir() && depth < MAX_DEPTH {
                        if let Some(directory) = path
                            .to_str()
                            .and_then(|path| probe.directory(path))
                            .filter(|path| within(path, &root))
                        {
                            pending.push((directory, depth + 1));
                        }
                    } else if metadata.is_file()
                        && path
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
                    {
                        let source = format!(
                            "{label}:{}",
                            path.strip_prefix(&root).unwrap_or(&path).display()
                        );
                        if let Some(shortcut) = load(&path, &source, probe) {
                            shortcuts.push(shortcut);
                        }
                    }
                }
                if exceeded {
                    break;
                }
            }
            // A truncated source must not select an arbitrary filesystem-order winner.
            if exceeded {
                shortcuts.truncate(start);
            }
        }
        shortcuts.sort_by_key(|shortcut| (path_key(&shortcut.target), shortcut.source.clone()));
        Ok(shortcuts)
    }

    #[cfg(test)]
    mod tests {
        use super::super::super::win32::NativePathProbe;
        use super::*;

        fn mutation_is_blocked_while_link_guard_is_alive(replace: bool) {
            struct Temp(PathBuf);
            impl Drop for Temp {
                fn drop(&mut self) {
                    let _ = fs::remove_dir_all(&self.0);
                }
            }
            let tree = Temp(std::env::temp_dir().join(format!(
                "logindeck-live-link-guard-test-{}",
                uuid::Uuid::new_v4()
            )));
            fs::create_dir(&tree.0).unwrap();
            let link = tree.0.join("Chat.lnk");
            let replacement = tree.0.join("replacement.lnk");
            fs::write(&link, b"initial!").unwrap();
            fs::write(&replacement, b"replacement").unwrap();
            let guard = pin_link(&link, &NativePathProbe).unwrap();
            assert_eq!(guard.file_size(), 8);
            // This is the precise production interval between the authoritative
            // bound in pin_link and IPersistFile::Load in load.
            let result = if replace {
                fs::rename(&replacement, &link)
            } else {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&link)
                    .and_then(|file| file.set_len(1_048_577))
            };
            assert!(
                matches!(
                    result.err().and_then(|error| error.raw_os_error()),
                    Some(5 | 32)
                ),
                "mutation must be denied while the guard is retained"
            );
            assert_eq!(guard.file_size(), 8);
            assert_eq!(fs::metadata(&link).unwrap().len(), 8);
            assert_eq!(fs::read(&link).unwrap(), b"initial!");
            // The retained guard must not lock unrelated files in this directory.
            let sibling = tree.0.join("sibling.txt");
            fs::write(&sibling, b"allowed").unwrap();
            fs::rename(&sibling, tree.0.join("renamed-sibling.txt")).unwrap();
            // Replacing the containing path must not redirect COM to a new link.
            let moved = tree.0.with_extension("moved");
            let move_parent = fs::rename(&tree.0, &moved);
            if move_parent.is_ok() {
                fs::rename(&moved, &tree.0).unwrap();
                panic!("containing path must not be replaceable while a link is pinned");
            }
            drop(guard);
            // Releasing the RAII guard restores the operation that was denied.
            if replace {
                fs::rename(&replacement, &link).unwrap();
            } else {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&link)
                    .unwrap()
                    .set_len(1_048_577)
                    .unwrap();
            }
        }

        #[test]
        fn link_guard_blocks_growth_until_com_read_finishes() {
            mutation_is_blocked_while_link_guard_is_alive(false);
        }

        #[test]
        fn link_guard_blocks_replacement_until_com_read_finishes() {
            mutation_is_blocked_while_link_guard_is_alive(true);
        }

        #[test]
        fn link_size_limit_uses_pinned_metadata_after_growth_or_replacement() {
            use crate::app_catalog::win32::SafeExecutable;
            use std::cell::Cell;
            struct Temp(PathBuf);
            impl Drop for Temp {
                fn drop(&mut self) {
                    let _ = fs::remove_dir_all(&self.0);
                }
            }
            struct ChangedLinkProbe {
                link: PathBuf,
                replace: bool,
                reached: Cell<bool>,
            }
            impl PathProbe for ChangedLinkProbe {
                fn directory(&self, value: &str) -> Option<PathBuf> {
                    self.reached.set(true);
                    if self.replace {
                        fs::rename(&self.link, self.link.with_extension("prior")).unwrap();
                        fs::write(&self.link, b"replacement").unwrap();
                    }
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&self.link)
                        .unwrap()
                        .set_len(1_048_577)
                        .unwrap();
                    NativePathProbe.directory(value)
                }
                fn executable(&self, _: &str) -> Option<SafeExecutable> {
                    panic!("size rejection must precede target resolution")
                }
                fn expand(&self, _: &str) -> Option<String> {
                    None
                }
                fn scan_install(&self, _: &Path, _: &str) -> Option<SafeExecutable> {
                    None
                }
            }
            let tree = Temp(
                std::env::temp_dir()
                    .join(format!("logindeck-link-size-test-{}", uuid::Uuid::new_v4())),
            );
            fs::create_dir(&tree.0).unwrap();
            for replace in [false, true] {
                let link = tree
                    .0
                    .join(if replace { "replace.lnk" } else { "grow.lnk" });
                fs::write(&link, b"small initial observation").unwrap();
                let probe = ChangedLinkProbe {
                    link: link.clone(),
                    replace,
                    reached: Cell::new(false),
                };
                let result = pin_link(&link, &probe);
                assert!(probe.reached.get());
                assert!(
                    result.is_none(),
                    "oversized pinned link must be rejected before COM Load"
                );
            }
        }

        #[test]
        fn com_link_fixture_reads_exact_target_and_rejects_arguments_without_launching() {
            struct Temp(PathBuf);
            impl Drop for Temp {
                fn drop(&mut self) {
                    let _ = fs::remove_dir_all(&self.0);
                }
            }
            let tree = Temp(
                std::env::temp_dir()
                    .join(format!("logindeck-shortcut-test-{}", uuid::Uuid::new_v4())),
            );
            fs::create_dir(&tree.0).unwrap();
            let target = tree.0.join("chat.exe");
            fs::write(&target, b"inert fixture - never executable").unwrap();
            let _apartment = Apartment::enter().unwrap();
            for (name, arguments, expected) in
                [("Chat", "", true), ("ChatWithArgs", "--arbitrary", false)]
            {
                let path = tree.0.join(format!("{name}.lnk"));
                let link: IShellLinkW =
                    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
                let wide_target: Vec<_> = target.as_os_str().encode_wide().chain([0]).collect();
                let wide_args: Vec<_> = arguments.encode_utf16().chain([0]).collect();
                let wide_path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
                unsafe { link.SetPath(PCWSTR(wide_target.as_ptr())) }.unwrap();
                unsafe { link.SetArguments(PCWSTR(wide_args.as_ptr())) }.unwrap();
                let persisted: IPersistFile = link.cast().unwrap();
                unsafe { persisted.Save(PCWSTR(wide_path.as_ptr()), true) }.unwrap();
                drop(persisted);
                drop(link);
                let actual = load(&path, "user-programs:Chat.lnk", &NativePathProbe);
                assert_eq!(actual.is_some(), expected);
                if let Some(actual) = actual {
                    let canonical = fs::canonicalize(&target).unwrap();
                    let canonical = canonical.to_str().unwrap();
                    assert_eq!(
                        actual.target,
                        Path::new(canonical.strip_prefix(r"\\?\").unwrap_or(canonical))
                    );
                }
                // The loader and COM object released their handles.
                fs::remove_file(&path).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn shortcut_buffers_require_a_terminator_and_valid_utf16() {
        assert_eq!(decode_buffer(&[65, 0, 66]), Some("A".into()));
        assert_eq!(decode_buffer(&[65, 66]), None);
        // The terminator is not last, so the invalid surrogate reaches decoding.
        assert_eq!(decode_buffer(&[0xd800, 0, 0]), None);
    }

    #[test]
    fn shortcut_valid_text_with_last_slot_terminator_is_rejected_as_saturated() {
        assert_eq!(decode_buffer(&[65, 0]), None);
    }

    #[test]
    fn raw_shortcuts_never_parse_arguments_or_accept_unsafe_targets() {
        assert!(parse_shortcut(
            "Chat",
            r"C:\Apps\Chat\chat.exe",
            "",
            "user-programs:Chat.lnk"
        )
        .is_some());
        for (target, args) in [
            (r"C:\Apps\Chat\chat.exe", "--launch-anything"),
            ("chat.exe", ""),
            (r"\\server\share\chat.exe", ""),
            (r"C:\Apps\chat.cmd", ""),
            (r"C:\Apps\chat.exe:stream", ""),
            (r"C:\Windows\System32\cmd.exe", ""),
            (r"C:\Apps\..\chat.exe", ""),
        ] {
            assert!(parse_shortcut("Chat", target, args, "user-programs:Chat.lnk").is_none());
        }
        assert!(parse_shortcut("", r"C:\Apps\Chat\chat.exe", "", "source").is_none());
        assert!(parse_shortcut(&"a".repeat(257), r"C:\Apps\Chat\chat.exe", "", "source").is_none());
    }

    #[test]
    fn shortcut_match_requires_install_anchor_and_name_with_stable_order() {
        let items = vec![
            Shortcut {
                name: "Chat".into(),
                target: r"C:\Apps\Chat\z.exe".into(),
                source: "common-programs:z.lnk".into(),
            },
            Shortcut {
                name: "Chat".into(),
                target: r"C:\Apps\ChatExtra\chat.exe".into(),
                source: "outside".into(),
            },
            Shortcut {
                name: "Other".into(),
                target: r"C:\Apps\Chat\other.exe".into(),
                source: "wrong-name".into(),
            },
            Shortcut {
                name: "chat".into(),
                target: r"C:\Apps\Chat\a.exe".into(),
                source: "user-programs:a.lnk".into(),
            },
        ];
        let matched = matching_shortcuts(&items, "Chat", Path::new(r"C:\Apps\Chat"));
        assert_eq!(
            matched
                .iter()
                .map(|item| item.source.as_str())
                .collect::<Vec<_>>(),
            vec!["user-programs:a.lnk", "common-programs:z.lnk"]
        );
    }
}
