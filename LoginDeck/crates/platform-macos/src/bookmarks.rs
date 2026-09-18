//! Opaque macOS security-scoped bookmark handling.
//!
//! Bookmark bytes are persisted as opaque access references. Resolution creates a stable
//! file-reference URL whose lifetime is tied to a Rust RAII object; the object is only created
//! and consumed inside the single blocking native verify-and-launch operation.

use std::path::Path;

use autologin_core::AppError;

pub(crate) trait BookmarkCreator: Send + Sync {
    fn create(&self, path: &Path) -> Result<String, AppError>;
}

pub(crate) struct MacBookmarks;

impl MacBookmarks {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
mod native {
    use std::{
        ffi::c_void,
        os::unix::ffi::OsStrExt,
        path::{Path, PathBuf},
        ptr,
    };

    use autologin_core::{AppError, ApplicationRecord};
    use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};

    use super::{BookmarkCreator, MacBookmarks};

    pub(crate) type CFTypeRef = *const c_void;
    pub(crate) type CFURLRef = *const c_void;
    type CFDataRef = *const c_void;
    type CFErrorRef = *const c_void;
    type CFIndex = isize;
    type Boolean = u8;

    // NSURLBookmarkCreationWithSecurityScope and NSURLBookmarkResolutionWithSecurityScope.
    const WITH_SECURITY_SCOPE: usize = 1 << 11;
    const RESOLVE_WITH_SECURITY_SCOPE: usize = 1 << 10;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFAllocatorDefault: CFTypeRef;
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFTypeRef,
            buffer: *const u8,
            buf_len: CFIndex,
            is_directory: Boolean,
        ) -> CFURLRef;
        fn CFURLCreateFileReferenceURL(
            allocator: CFTypeRef,
            url: CFURLRef,
            error: *mut CFErrorRef,
        ) -> CFURLRef;
        fn CFURLCreateBookmarkData(
            allocator: CFTypeRef,
            url: CFURLRef,
            options: usize,
            properties_to_include: CFTypeRef,
            relative_to_url: CFURLRef,
            error: *mut CFErrorRef,
        ) -> CFDataRef;
        fn CFURLCreateByResolvingBookmarkData(
            allocator: CFTypeRef,
            bookmark: CFDataRef,
            options: usize,
            relative_to_url: CFURLRef,
            properties_to_include: CFTypeRef,
            is_stale: *mut Boolean,
            error: *mut CFErrorRef,
        ) -> CFURLRef;
        fn CFDataCreate(allocator: CFTypeRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
        fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
        fn CFDataGetLength(data: CFDataRef) -> CFIndex;
        fn CFURLCopyFileSystemPath(url: CFURLRef, path_style: i32) -> CFTypeRef;
        fn CFGetTypeID(value: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetCString(
            value: CFTypeRef,
            buffer: *mut i8,
            buffer_size: CFIndex,
            encoding: u32,
        ) -> Boolean;
        fn CFRelease(value: CFTypeRef);
    }
    #[link(name = "objc")]
    extern "C" {
        fn sel_registerName(name: *const std::ffi::c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    fn unavailable() -> AppError {
        AppError::new("application.access_unavailable")
    }

    unsafe fn start_scope(url: CFURLRef) -> bool {
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u8 =
            std::mem::transmute(objc_msgSend as *const ());
        send(
            url.cast_mut(),
            sel_registerName(c"startAccessingSecurityScopedResource".as_ptr()),
        ) != 0
    }
    unsafe fn stop_scope(url: CFURLRef) {
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void) =
            std::mem::transmute(objc_msgSend as *const ());
        send(
            url.cast_mut(),
            sel_registerName(c"stopAccessingSecurityScopedResource".as_ptr()),
        );
    }

    unsafe fn file_url(path: &Path) -> Result<CFURLRef, AppError> {
        let bytes = path.as_os_str().as_bytes();
        let url = CFURLCreateFromFileSystemRepresentation(
            kCFAllocatorDefault,
            bytes.as_ptr(),
            bytes.len() as CFIndex,
            path.is_dir() as Boolean,
        );
        if url.is_null() {
            Err(AppError::application_not_found())
        } else {
            Ok(url)
        }
    }
    unsafe fn stable_url(url: CFURLRef) -> Result<CFURLRef, AppError> {
        let mut error = ptr::null();
        let stable = CFURLCreateFileReferenceURL(kCFAllocatorDefault, url, &mut error);
        if !error.is_null() {
            CFRelease(error);
        }
        if stable.is_null() {
            Err(AppError::application_not_found())
        } else {
            Ok(stable)
        }
    }
    unsafe fn path_from_url(url: CFURLRef) -> Result<PathBuf, AppError> {
        let string = CFURLCopyFileSystemPath(url, 0);
        if string.is_null() || CFGetTypeID(string) != CFStringGetTypeID() {
            if !string.is_null() {
                CFRelease(string);
            }
            return Err(unavailable());
        }
        let mut bytes = vec![0_i8; 4096];
        let ok = CFStringGetCString(string, bytes.as_mut_ptr(), bytes.len() as CFIndex, UTF8);
        CFRelease(string);
        if ok == 0 {
            return Err(unavailable());
        }
        let nul = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(unavailable)?;
        let value = std::str::from_utf8(std::slice::from_raw_parts(bytes.as_ptr().cast(), nul))
            .map_err(|_| unavailable())?;
        Ok(PathBuf::from(value))
    }

    /// A stable URL and, when applicable, its active security scope. Drop always stops/releases
    /// the scope, including cancellation/panic unwinding inside the native blocking closure.
    pub(crate) struct StableApplicationUrl {
        url: CFURLRef,
        scope_url: Option<CFURLRef>,
        path: PathBuf,
    }
    impl StableApplicationUrl {
        pub(crate) fn url(&self) -> CFURLRef {
            self.url
        }
        pub(crate) fn path(&self) -> &Path {
            &self.path
        }
    }
    impl Drop for StableApplicationUrl {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.url);
                if let Some(scope_url) = self.scope_url.take() {
                    stop_scope(scope_url);
                    CFRelease(scope_url);
                }
            }
        }
    }

    pub(crate) fn stable_application_url(
        app: &ApplicationRecord,
    ) -> Result<StableApplicationUrl, AppError> {
        unsafe {
            let (resolved, scope_url) = match app.path_access_ref.as_deref() {
                Some(access_ref) => {
                    let bytes = STANDARD_NO_PAD
                        .decode(access_ref)
                        .map_err(|_| unavailable())?;
                    let data =
                        CFDataCreate(kCFAllocatorDefault, bytes.as_ptr(), bytes.len() as CFIndex);
                    if data.is_null() {
                        return Err(unavailable());
                    }
                    let mut stale = 0;
                    let mut error = ptr::null();
                    let resolved = CFURLCreateByResolvingBookmarkData(
                        kCFAllocatorDefault,
                        data,
                        RESOLVE_WITH_SECURITY_SCOPE,
                        ptr::null(),
                        ptr::null(),
                        &mut stale,
                        &mut error,
                    );
                    CFRelease(data);
                    if !error.is_null() {
                        CFRelease(error);
                    }
                    if resolved.is_null() || stale != 0 {
                        if !resolved.is_null() {
                            CFRelease(resolved);
                        }
                        return Err(AppError::application_not_found());
                    }
                    if !start_scope(resolved) {
                        CFRelease(resolved);
                        return Err(unavailable());
                    }
                    (resolved, Some(resolved))
                }
                None => (file_url(Path::new(&app.launch_target))?, None),
            };
            let stable = match stable_url(resolved) {
                Ok(stable) => stable,
                Err(error) => {
                    if scope_url.is_some() {
                        stop_scope(resolved);
                    }
                    CFRelease(resolved);
                    return Err(error);
                }
            };
            if scope_url.is_none() {
                CFRelease(resolved);
            }
            let path = match path_from_url(stable) {
                Ok(path) => path,
                Err(error) => {
                    CFRelease(stable);
                    if let Some(scope_url) = scope_url {
                        stop_scope(scope_url);
                        CFRelease(scope_url);
                    }
                    return Err(error);
                }
            };
            Ok(StableApplicationUrl {
                url: stable,
                scope_url,
                path,
            })
        }
    }

    impl BookmarkCreator for MacBookmarks {
        fn create(&self, path: &Path) -> Result<String, AppError> {
            unsafe {
                let url = file_url(path)?;
                let mut error = ptr::null();
                let data = CFURLCreateBookmarkData(
                    kCFAllocatorDefault,
                    url,
                    WITH_SECURITY_SCOPE,
                    ptr::null(),
                    ptr::null(),
                    &mut error,
                );
                CFRelease(url);
                if data.is_null() {
                    if !error.is_null() {
                        CFRelease(error);
                    }
                    return Err(unavailable());
                }
                let bytes = std::slice::from_raw_parts(
                    CFDataGetBytePtr(data),
                    CFDataGetLength(data) as usize,
                );
                let encoded = STANDARD_NO_PAD.encode(bytes);
                CFRelease(data);
                Ok(encoded)
            }
        }
    }

    #[cfg(test)]
    mod tests {
        // Header/link regression: stop is intentionally modeled as void, not BOOL.
        use super::stop_scope;
        #[test]
        fn stop_scope_has_void_abi() {
            let _: unsafe fn(super::CFURLRef) = stop_scope;
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use native::stable_application_url;

#[cfg(not(target_os = "macos"))]
impl BookmarkCreator for MacBookmarks {
    fn create(&self, _path: &Path) -> Result<String, AppError> {
        Err(AppError::new("application.access_unavailable"))
    }
}
