use std::sync::Arc;

use async_trait::async_trait;
use autologin_core::{AppError, Clipboard};
use secrecy::SecretString;

/// System pasteboard boundary. `clear_if_change_count` is one operation so callers cannot turn a
/// token into a general clipboard read or an arbitrary clear request.
#[async_trait]
trait PasteboardBackend: Send + Sync {
    async fn write(&self, value: SecretString) -> Result<u64, AppError>;
    async fn clear_if_change_count(&self, change_count: u64) -> Result<(), AppError>;
}

#[async_trait]
impl<T> PasteboardBackend for Arc<T>
where
    T: PasteboardBackend + ?Sized,
{
    async fn write(&self, value: SecretString) -> Result<u64, AppError> {
        (**self).write(value).await
    }

    async fn clear_if_change_count(&self, change_count: u64) -> Result<(), AppError> {
        (**self).clear_if_change_count(change_count).await
    }
}

/// Opaque proof that this process performed one specific pasteboard write.
#[derive(Eq, PartialEq)]
pub struct MacPasteboardChangeToken(u64);

impl std::fmt::Debug for MacPasteboardChangeToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MacPasteboardChangeToken(<opaque>)")
    }
}

/// Bounded macOS clipboard implementation for password copies.
///
/// It does not expose a clipboard read API. The native backend records `NSPasteboard.changeCount`
/// after its write and checks it immediately before clearing; if another app has copied anything,
/// the count differs and the later content is preserved. AppKit has no compare-and-clear API, so
/// this is the strongest native condition available; the backend keeps the check and clear
/// adjacent and the injectable backend makes the condition atomic for tests.
pub struct MacClipboard {
    backend: Arc<dyn PasteboardBackend>,
}

impl MacClipboard {
    pub fn new() -> Self {
        Self::from_backend(MacPasteboardBackend)
    }

    fn from_backend<B>(backend: B) -> Self
    where
        B: PasteboardBackend + 'static,
    {
        Self {
            backend: Arc::new(backend),
        }
    }

    #[cfg(test)]
    fn with_backend<B>(backend: B) -> Self
    where
        B: PasteboardBackend + 'static,
    {
        Self::from_backend(backend)
    }
}

impl Default for MacClipboard {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Clipboard for MacClipboard {
    type ChangeToken = MacPasteboardChangeToken;

    async fn write(&self, value: SecretString) -> Result<Self::ChangeToken, AppError> {
        self.backend
            .write(value)
            .await
            .map(MacPasteboardChangeToken)
    }

    async fn clear_if_unchanged(&self, token: &Self::ChangeToken) -> Result<(), AppError> {
        self.backend.clear_if_change_count(token.0).await
    }
}

struct MacPasteboardBackend;

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::CString;

    use autologin_core::AppError;
    use core_foundation::{base::TCFType, string::CFString};
    use secrecy::{ExposeSecret, SecretString};

    use super::{MacPasteboardBackend, PasteboardBackend};

    type Id = *mut std::ffi::c_void;
    type Sel = *mut std::ffi::c_void;

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const std::ffi::c_char) -> Id;
        fn sel_registerName(name: *const std::ffi::c_char) -> Sel;
        fn objc_msgSend();
    }

    fn class(name: &str) -> Result<Id, AppError> {
        let name = CString::new(name).expect("Objective-C class names contain no NUL");
        let value = unsafe { objc_getClass(name.as_ptr()) };
        if value.is_null() {
            Err(AppError::new("clipboard.unavailable"))
        } else {
            Ok(value)
        }
    }

    fn selector(name: &str) -> Sel {
        let name = CString::new(name).expect("Objective-C selectors contain no NUL");
        unsafe { sel_registerName(name.as_ptr()) }
    }

    unsafe fn send_id(receiver: Id, selector: Sel) -> Id {
        let send: unsafe extern "C" fn(Id, Sel) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        send(receiver, selector)
    }

    unsafe fn send_integer(receiver: Id, selector: Sel) -> isize {
        let send: unsafe extern "C" fn(Id, Sel) -> isize =
            std::mem::transmute(objc_msgSend as *const ());
        send(receiver, selector)
    }

    unsafe fn send_bool_id_id(receiver: Id, selector: Sel, first: Id, second: Id) -> bool {
        let send: unsafe extern "C" fn(Id, Sel, Id, Id) -> bool =
            std::mem::transmute(objc_msgSend as *const ());
        send(receiver, selector, first, second)
    }

    fn pasteboard() -> Result<Id, AppError> {
        let class = class("NSPasteboard")?;
        let value = unsafe { send_id(class, selector("generalPasteboard")) };
        if value.is_null() {
            Err(AppError::new("clipboard.unavailable"))
        } else {
            Ok(value)
        }
    }

    #[async_trait::async_trait]
    impl PasteboardBackend for MacPasteboardBackend {
        async fn write(&self, value: SecretString) -> Result<u64, AppError> {
            let pasteboard = pasteboard()?;
            let string = CFString::new(value.expose_secret());
            let string_type = CFString::new("public.utf8-plain-text");

            // clearContents invalidates the prior pasteboard owner; the write then has a distinct
            // change count used as the conditional-clear token.
            unsafe { send_integer(pasteboard, selector("clearContents")) };
            let wrote = unsafe {
                send_bool_id_id(
                    pasteboard,
                    selector("setString:forType:"),
                    string.as_CFTypeRef() as Id,
                    string_type.as_CFTypeRef() as Id,
                )
            };
            if !wrote {
                return Err(AppError::new("clipboard.unavailable"));
            }
            let change_count = unsafe { send_integer(pasteboard, selector("changeCount")) };
            u64::try_from(change_count).map_err(|_| AppError::new("clipboard.unavailable"))
        }

        async fn clear_if_change_count(&self, expected: u64) -> Result<(), AppError> {
            let pasteboard = pasteboard()?;
            let observed = unsafe { send_integer(pasteboard, selector("changeCount")) };
            if u64::try_from(observed).ok() == Some(expected) {
                // No clipboard contents are read. This follows the native change-count guard and
                // is intentionally a no-op if another application wrote after our password copy.
                unsafe { send_integer(pasteboard, selector("clearContents")) };
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn pasteboard_symbols_link_the_appkit_framework_explicitly() {
            assert!(include_str!("clipboard.rs")
                .contains("#[link(name = \"AppKit\", kind = \"framework\")]"));
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[async_trait]
impl PasteboardBackend for MacPasteboardBackend {
    async fn write(&self, _value: SecretString) -> Result<u64, AppError> {
        Err(AppError::new("platform.unsupported"))
    }

    async fn clear_if_change_count(&self, _change_count: u64) -> Result<(), AppError> {
        Err(AppError::new("platform.unsupported"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{MacClipboard, PasteboardBackend};
    use async_trait::async_trait;
    use autologin_core::{AppError, Clipboard};
    use secrecy::{ExposeSecret, SecretString};

    fn secret(value: &str) -> SecretString {
        SecretString::from(value.to_owned())
    }

    #[derive(Default)]
    struct FakePasteboard {
        state: Mutex<PasteboardState>,
    }

    #[derive(Default)]
    struct PasteboardState {
        value: Option<SecretString>,
        change_count: u64,
    }

    impl FakePasteboard {
        fn user_copies(&self, value: &str) {
            let mut state = self.state.lock().expect("fake pasteboard lock");
            state.value = Some(secret(value));
            state.change_count += 1;
        }

        fn contents(&self) -> Option<String> {
            self.state
                .lock()
                .expect("fake pasteboard lock")
                .value
                .as_ref()
                .map(|value| value.expose_secret().to_owned())
        }
    }

    #[async_trait]
    impl PasteboardBackend for FakePasteboard {
        async fn write(&self, value: SecretString) -> Result<u64, AppError> {
            let mut state = self.state.lock().expect("fake pasteboard lock");
            state.value = Some(value);
            state.change_count += 1;
            Ok(state.change_count)
        }

        async fn clear_if_change_count(&self, change_count: u64) -> Result<(), AppError> {
            let mut state = self.state.lock().expect("fake pasteboard lock");
            if state.change_count == change_count {
                state.value = None;
                state.change_count += 1;
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn clipboard_clears_only_its_unchanged_write() {
        let backend = Arc::new(FakePasteboard::default());
        let clipboard = MacClipboard::with_backend(backend.clone());
        let token = clipboard.write(secret("hunter2")).await.unwrap();

        clipboard.clear_if_unchanged(&token).await.unwrap();

        assert_eq!(backend.contents(), None);
    }

    #[tokio::test]
    async fn clipboard_does_not_clear_a_later_user_copy() {
        let backend = Arc::new(FakePasteboard::default());
        let clipboard = MacClipboard::with_backend(backend.clone());
        let token = clipboard.write(secret("hunter2")).await.unwrap();
        backend.user_copies("new user content");

        clipboard.clear_if_unchanged(&token).await.unwrap();

        assert_eq!(backend.contents().as_deref(), Some("new user content"));
    }
}
