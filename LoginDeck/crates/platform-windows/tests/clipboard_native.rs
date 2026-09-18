#![cfg(target_os = "windows")]

use std::{ptr, slice, thread, time::Duration};

use autologin_core::Clipboard;
use platform_windows::WindowsClipboard;
use secrecy::{ExposeSecret, SecretString};
use windows::{
    core::w,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND, WAIT_ABANDONED, WAIT_OBJECT_0},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
                IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
            Ole::CF_UNICODETEXT,
            Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject, INFINITE},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
        },
    },
};

const OPEN_ATTEMPTS: usize = 10;
const CF_UNICODETEXT_ID: u32 = CF_UNICODETEXT.0 as u32;

struct ProcessTestLock(HANDLE);

impl ProcessTestLock {
    fn acquire() -> Self {
        let handle =
            unsafe { CreateMutexW(None, false, w!("Local\\LoginDeck.Clipboard.NativeTests")) }
                .expect("test mutex must be available");
        let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
        assert!(
            wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED,
            "test mutex wait failed"
        );
        Self(handle)
    }
}

impl Drop for ProcessTestLock {
    fn drop(&mut self) {
        let _ = unsafe { ReleaseMutex(self.0) };
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct OwnerWindow(HWND);

impl OwnerWindow {
    fn create() -> windows::core::Result<Self> {
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }?;
        Ok(Self(window))
    }
}

impl Drop for OwnerWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

struct OpenClipboardGuard;

impl OpenClipboardGuard {
    fn open(owner: Option<HWND>) -> windows::core::Result<Self> {
        for attempt in 0..OPEN_ATTEMPTS {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Self);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                thread::sleep(Duration::from_millis(5));
            }
        }
        Err(windows::core::Error::from_win32())
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

struct OwnedGlobal(Option<HGLOBAL>);

impl OwnedGlobal {
    fn unicode(value: &str) -> windows::core::Result<Self> {
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = wide
            .len()
            .checked_mul(size_of::<u16>())
            .ok_or_else(windows::core::Error::from_win32)?;
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }?;
        let owned = Self(Some(memory));
        let destination = unsafe { GlobalLock(memory) };
        if destination.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        unsafe {
            ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), destination.cast(), byte_len);
            // A successful final unlock returns zero by design; only the lock's non-null result is
            // required for this single-lock allocation.
            let _ = GlobalUnlock(memory);
        }
        Ok(owned)
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.0.expect("owned global must be armed").0)
    }

    fn transfer(mut self) {
        self.0 = None;
    }
}

impl Drop for OwnedGlobal {
    fn drop(&mut self) {
        if let Some(memory) = self.0.take() {
            let _ = unsafe { windows::Win32::Foundation::GlobalFree(Some(memory)) };
        }
    }
}

fn set_unicode_while_open(value: &str) -> windows::core::Result<()> {
    unsafe { EmptyClipboard() }?;
    let memory = OwnedGlobal::unicode(value)?;
    unsafe { SetClipboardData(CF_UNICODETEXT_ID, Some(memory.handle())) }?;
    memory.transfer();
    Ok(())
}

fn native_write(value: &str) -> windows::core::Result<u32> {
    let owner = OwnerWindow::create()?;
    let _open = OpenClipboardGuard::open(Some(owner.0))?;
    set_unicode_while_open(value)?;
    Ok(unsafe { GetClipboardSequenceNumber() })
}

fn read_unicode_while_open() -> windows::core::Result<Option<SecretString>> {
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT_ID) }.is_err() {
        return Ok(None);
    }
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT_ID) }?;
    let memory = HGLOBAL(handle.0);
    let pointer = unsafe { GlobalLock(memory) };
    if pointer.is_null() {
        return Err(windows::core::Error::from_win32());
    }
    let unit_count = unsafe { GlobalSize(memory) } / size_of::<u16>();
    let units = unsafe { slice::from_raw_parts(pointer.cast::<u16>(), unit_count) };
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(unit_count);
    let result = String::from_utf16(&units[..end])
        .map(SecretString::from)
        .map_err(|_| windows::core::Error::from_win32());
    unsafe {
        let _ = GlobalUnlock(memory);
    }
    result.map(Some)
}

fn inspect_unicode() -> windows::core::Result<(Option<SecretString>, u32)> {
    let _open = OpenClipboardGuard::open(None)?;
    let value = read_unicode_while_open()?;
    let sequence = unsafe { GetClipboardSequenceNumber() };
    Ok((value, sequence))
}

struct ClipboardSnapshot {
    original: Option<SecretString>,
    last_fixture_sequence: Option<u32>,
}

impl ClipboardSnapshot {
    fn capture() -> Self {
        let (original, _) = inspect_unicode().expect("clipboard snapshot must be readable");
        Self {
            original,
            last_fixture_sequence: None,
        }
    }

    fn expect_exact(&mut self, expected: &str) {
        let (actual, sequence) = inspect_unicode().expect("clipboard fixture must be readable");
        assert!(
            actual
                .as_ref()
                .is_some_and(|value| value.expose_secret() == expected),
            "clipboard did not contain the expected fixture"
        );
        self.last_fixture_sequence = Some(sequence);
    }

    fn expect_empty(&mut self) {
        let (actual, sequence) = inspect_unicode().expect("clipboard fixture must be readable");
        assert!(
            actual.is_none(),
            "Unicode clipboard data remained available"
        );
        self.last_fixture_sequence = Some(sequence);
    }
}

impl Drop for ClipboardSnapshot {
    fn drop(&mut self) {
        let Some(expected) = self.last_fixture_sequence else {
            return;
        };
        if unsafe { GetClipboardSequenceNumber() } != expected {
            return;
        }
        let Ok(owner) = OwnerWindow::create() else {
            return;
        };
        let Ok(_open) = OpenClipboardGuard::open(Some(owner.0)) else {
            return;
        };
        if unsafe { GetClipboardSequenceNumber() } != expected {
            return;
        }
        match self.original.as_ref() {
            Some(original) => {
                let _ = set_unicode_while_open(original.expose_secret());
            }
            None => {
                let _ = unsafe { EmptyClipboard() };
            }
        }
    }
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

#[tokio::test]
async fn own_unchanged_write_can_be_cleared() {
    let _process = ProcessTestLock::acquire();
    let mut snapshot = ClipboardSnapshot::capture();
    let clipboard = WindowsClipboard::new();

    let token = clipboard.write(secret("LoginDeck fixture")).await.unwrap();
    snapshot.expect_exact("LoginDeck fixture");
    clipboard.clear_if_unchanged(&token).await.unwrap();

    snapshot.expect_empty();
    drop(snapshot);
    drop(clipboard);
}

#[tokio::test]
async fn replacement_by_an_independent_native_writer_is_preserved() {
    let _process = ProcessTestLock::acquire();
    let mut snapshot = ClipboardSnapshot::capture();
    let clipboard = WindowsClipboard::new();

    let token = clipboard.write(secret("LoginDeck old")).await.unwrap();
    native_write("user replacement").expect("independent write must succeed");
    snapshot.expect_exact("user replacement");
    clipboard.clear_if_unchanged(&token).await.unwrap();

    snapshot.expect_exact("user replacement");
    drop(snapshot);
    drop(clipboard);
}

#[tokio::test]
async fn first_of_two_logindeck_tokens_cannot_clear_the_second_write() {
    let _process = ProcessTestLock::acquire();
    let mut snapshot = ClipboardSnapshot::capture();
    let clipboard = WindowsClipboard::new();

    let first = clipboard.write(secret("LoginDeck first")).await.unwrap();
    let second = clipboard.write(secret("LoginDeck second")).await.unwrap();
    clipboard.clear_if_unchanged(&first).await.unwrap();
    snapshot.expect_exact("LoginDeck second");

    clipboard.clear_if_unchanged(&second).await.unwrap();
    snapshot.expect_empty();
    drop(snapshot);
    drop(clipboard);
}

#[tokio::test]
async fn embedded_nul_is_rejected_before_clipboard_mutation() {
    let _process = ProcessTestLock::acquire();
    let mut snapshot = ClipboardSnapshot::capture();
    native_write("unchanged marker").expect("marker write must succeed");
    snapshot.expect_exact("unchanged marker");
    let clipboard = WindowsClipboard::new();

    let error = clipboard
        .write(secret("secret\0suffix"))
        .await
        .expect_err("embedded NUL must fail validation");

    assert_eq!(error.code(), "validation.invalid_field");
    assert_eq!(
        error.params.get("field").map(String::as_str),
        Some("password")
    );
    snapshot.expect_exact("unchanged marker");
    drop(snapshot);
    drop(clipboard);
}
