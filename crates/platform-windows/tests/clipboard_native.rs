#![cfg(target_os = "windows")]

use std::{
    future::Future,
    pin::Pin,
    ptr, slice,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
    time::{Duration, Instant},
};

use autologin_core::Clipboard;
use platform_windows::WindowsClipboard;
use secrecy::{ExposeSecret, SecretString};
use windows::{
    core::w,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND, WAIT_ABANDONED, WAIT_OBJECT_0},
        System::{
            DataExchange::{
                CloseClipboard, CountClipboardFormats, EmptyClipboard, GetClipboardData,
                GetClipboardOwner, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
                OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
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

struct OpenClipboardGuard {
    open: bool,
}

impl OpenClipboardGuard {
    fn open(owner: Option<HWND>) -> windows::core::Result<Self> {
        for attempt in 0..OPEN_ATTEMPTS {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Self { open: true });
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                thread::sleep(Duration::from_millis(5));
            }
        }
        Err(windows::core::Error::from_win32())
    }

    fn close(mut self) -> windows::core::Result<()> {
        unsafe { CloseClipboard() }?;
        self.open = false;
        Ok(())
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        if self.open {
            let _ = unsafe { CloseClipboard() };
        }
    }
}

struct OwnedGlobal(Option<HGLOBAL>);

impl OwnedGlobal {
    fn bytes(value: &[u8]) -> windows::core::Result<Self> {
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, value.len()) }?;
        let owned = Self(Some(memory));
        let destination = unsafe { GlobalLock(memory) };
        if destination.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        unsafe {
            ptr::copy_nonoverlapping(value.as_ptr(), destination.cast(), value.len());
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
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes =
        unsafe { slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * size_of::<u16>()) };
    let memory = OwnedGlobal::bytes(bytes)?;
    unsafe { SetClipboardData(CF_UNICODETEXT_ID, Some(memory.handle())) }?;
    memory.transfer();
    Ok(())
}

fn set_format_while_open(format: u32, value: &[u8]) -> windows::core::Result<()> {
    unsafe { EmptyClipboard() }?;
    let memory = OwnedGlobal::bytes(value)?;
    unsafe { SetClipboardData(format, Some(memory.handle())) }?;
    memory.transfer();
    Ok(())
}

struct OwnedTestMutation {
    owner: OwnerWindow,
    sequence: u32,
}

impl OwnedTestMutation {
    fn apply(mutate: impl FnOnce() -> windows::core::Result<()>) -> windows::core::Result<Self> {
        let owner = OwnerWindow::create()?;
        let open = OpenClipboardGuard::open(Some(owner.0))?;
        mutate()?;
        open.close()?;
        let sequence = unsafe { GetClipboardSequenceNumber() };
        let observed_owner = unsafe { GetClipboardOwner() }?;
        if observed_owner != owner.0 {
            return Err(windows::core::Error::from_win32());
        }
        Ok(Self { owner, sequence })
    }

    fn unicode(value: &str) -> windows::core::Result<Self> {
        Self::apply(|| set_unicode_while_open(value))
    }

    fn empty() -> windows::core::Result<Self> {
        Self::apply(|| unsafe { EmptyClipboard() })
    }

    fn non_text(format: u32, value: &[u8]) -> windows::core::Result<Self> {
        Self::apply(|| set_format_while_open(format, value))
    }
}

fn native_write(value: &str) -> windows::core::Result<OwnedTestMutation> {
    OwnedTestMutation::unicode(value)
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

fn inspect_unicode() -> windows::core::Result<Option<SecretString>> {
    let _open = OpenClipboardGuard::open(None)?;
    read_unicode_while_open()
}

fn format_available(format: u32) -> windows::core::Result<bool> {
    let _open = OpenClipboardGuard::open(None)?;
    Ok(unsafe { IsClipboardFormatAvailable(format) }.is_ok())
}

fn non_text_format() -> u32 {
    let format = unsafe { RegisterClipboardFormatW(w!("LoginDeck.Tests.ImageLike")) };
    assert!(format != 0, "test clipboard format registration failed");
    format
}

struct ClipboardSnapshot {
    original: Option<SecretString>,
    owned_mutation: Option<OwnedTestMutation>,
}

impl ClipboardSnapshot {
    fn capture() -> Self {
        let original = inspect_unicode().expect("clipboard snapshot must be readable");
        Self {
            original,
            owned_mutation: None,
        }
    }

    fn expect_exact(&self, expected: &str) {
        let actual = inspect_unicode().expect("clipboard fixture must be readable");
        assert!(
            actual
                .as_ref()
                .is_some_and(|value| value.expose_secret() == expected),
            "clipboard did not contain the expected fixture"
        );
    }

    fn expect_empty(&self) {
        let _open = OpenClipboardGuard::open(None).expect("clipboard must be readable");
        assert!(
            unsafe { CountClipboardFormats() } == 0,
            "clipboard still contains one or more formats"
        );
    }

    fn claim(&mut self, mutation: OwnedTestMutation) {
        self.owned_mutation = Some(mutation);
    }
}

impl Drop for ClipboardSnapshot {
    fn drop(&mut self) {
        let Some(mutation) = self.owned_mutation.as_ref() else {
            return;
        };
        let expected = mutation.sequence;
        if unsafe { GetClipboardSequenceNumber() } != expected {
            return;
        }
        let Ok(_open) = OpenClipboardGuard::open(Some(mutation.owner.0)) else {
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

struct NoWake;

impl Wake for NoWake {
    fn wake(self: Arc<Self>) {}
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    let waker = Waker::from(Arc::new(NoWake));
    future.poll(&mut Context::from_waker(&waker))
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

#[tokio::test]
async fn own_unchanged_write_can_be_cleared() {
    let _process = ProcessTestLock::acquire();
    let snapshot = ClipboardSnapshot::capture();
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
    let replacement = native_write("user replacement").expect("independent write must succeed");
    snapshot.expect_exact("user replacement");
    clipboard.clear_if_unchanged(&token).await.unwrap();

    snapshot.expect_exact("user replacement");
    snapshot.claim(replacement);
    drop(snapshot);
    drop(clipboard);
}

#[tokio::test]
async fn first_of_two_logindeck_tokens_cannot_clear_the_second_write() {
    let _process = ProcessTestLock::acquire();
    let snapshot = ClipboardSnapshot::capture();
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
    let marker = native_write("unchanged marker").expect("marker write must succeed");
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
    snapshot.claim(marker);
    drop(snapshot);
    drop(clipboard);
}

#[test]
fn teardown_does_not_overwrite_a_later_non_text_mutation() {
    let _process = ProcessTestLock::acquire();
    let mut restore_user_clipboard = ClipboardSnapshot::capture();
    let mut subject = ClipboardSnapshot::capture();
    subject.claim(
        OwnedTestMutation::unicode("LoginDeck owned mutation")
            .expect("owned fixture write must succeed"),
    );

    let format = non_text_format();
    let replacement = OwnedTestMutation::non_text(format, b"image-like-test-bytes")
        .expect("non-text replacement must succeed");
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| subject.expect_empty())).is_err(),
        "non-text clipboard content was incorrectly treated as empty"
    );
    drop(subject);

    assert!(
        format_available(format).expect("format check must succeed"),
        "teardown overwrote a later non-text clipboard mutation"
    );
    restore_user_clipboard.claim(replacement);
    drop(restore_user_clipboard);
}

#[test]
fn teardown_does_not_adopt_identical_text_from_a_later_writer() {
    let _process = ProcessTestLock::acquire();
    let mut restore_user_clipboard = ClipboardSnapshot::capture();
    let baseline =
        OwnedTestMutation::unicode("subject original").expect("subject baseline must succeed");
    let mut subject = ClipboardSnapshot::capture();
    subject.claim(
        OwnedTestMutation::unicode("identical replacement")
            .expect("owned fixture write must succeed"),
    );

    let replacement = OwnedTestMutation::unicode("identical replacement")
        .expect("later identical write must succeed");
    drop(subject);

    ClipboardSnapshot::capture().expect_exact("identical replacement");
    restore_user_clipboard.claim(replacement);
    drop(restore_user_clipboard);
    drop(baseline);
}

#[tokio::test]
async fn cancellation_before_publication_cleans_the_unclaimed_write() {
    let _process = ProcessTestLock::acquire();
    let snapshot = ClipboardSnapshot::capture();
    let clipboard = WindowsClipboard::new();
    let stale = clipboard.write(secret("stale token")).await.unwrap();
    drop(OwnedTestMutation::empty().expect("empty baseline must succeed"));

    let owner = OwnerWindow::create().expect("blocking owner must be created");
    let held = OpenClipboardGuard::open(Some(owner.0)).expect("clipboard hold must succeed");
    let mut cancelled = Box::pin(clipboard.write(secret("cancel before publication")));
    assert!(matches!(poll_once(cancelled.as_mut()), Poll::Pending));
    drop(cancelled);
    drop(held);
    drop(owner);

    // Two FIFO barriers cover either ordering between the already-queued barrier and the cleanup
    // command created when publication discovers that its receiver was cancelled.
    clipboard.clear_if_unchanged(&stale).await.unwrap();
    clipboard.clear_if_unchanged(&stale).await.unwrap();
    snapshot.expect_empty();
    drop(snapshot);
    drop(clipboard);
}

#[tokio::test]
async fn cancellation_after_successful_result_send_cleans_the_unclaimed_write() {
    let _process = ProcessTestLock::acquire();
    let snapshot = ClipboardSnapshot::capture();
    let clipboard = WindowsClipboard::new();
    let stale = clipboard.write(secret("stale token")).await.unwrap();
    let mut cancelled = Box::pin(clipboard.write(secret("cancel after send")));
    assert!(matches!(poll_once(cancelled.as_mut()), Poll::Pending));

    // Same-worker FIFO completion proves the prior write command has published its result into the
    // still-unpolled oneshot receiver.
    clipboard.clear_if_unchanged(&stale).await.unwrap();
    snapshot.expect_exact("cancel after send");
    drop(cancelled);
    clipboard.clear_if_unchanged(&stale).await.unwrap();

    snapshot.expect_empty();
    drop(snapshot);
    drop(clipboard);
}

#[tokio::test]
async fn cancelled_published_write_preserves_a_later_replacement() {
    let _process = ProcessTestLock::acquire();
    let mut snapshot = ClipboardSnapshot::capture();
    let clipboard = WindowsClipboard::new();
    let stale = clipboard.write(secret("stale token")).await.unwrap();
    let mut cancelled = Box::pin(clipboard.write(secret("unclaimed password")));
    assert!(matches!(poll_once(cancelled.as_mut()), Poll::Pending));
    clipboard.clear_if_unchanged(&stale).await.unwrap();

    let replacement =
        native_write("user replacement after publish").expect("replacement must succeed");
    drop(cancelled);
    clipboard.clear_if_unchanged(&stale).await.unwrap();

    snapshot.expect_exact("user replacement after publish");
    snapshot.claim(replacement);
    drop(snapshot);
    drop(clipboard);
}

#[tokio::test]
async fn two_instances_serialize_without_blocking_the_owner_message_pump() {
    let _process = ProcessTestLock::acquire();
    let snapshot = ClipboardSnapshot::capture();
    let first = WindowsClipboard::new();
    let second = WindowsClipboard::new();
    let _owner_token = first.write(secret("first owner")).await.unwrap();

    let owner = OwnerWindow::create().expect("blocking owner must be created");
    let held = OpenClipboardGuard::open(Some(owner.0)).expect("clipboard hold must succeed");
    let mut second_write = Box::pin(second.write(secret("second instance")));
    assert!(matches!(poll_once(second_write.as_mut()), Poll::Pending));
    thread::sleep(Duration::from_millis(8));
    let mut first_write = Box::pin(first.write(secret("first instance final")));
    assert!(matches!(poll_once(first_write.as_mut()), Poll::Pending));

    let started = Instant::now();
    drop(held);
    drop(owner);
    let second_token = second_write.await.unwrap();
    let first_token = first_write.await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "owner message pumping was blocked across clipboard instances"
    );

    second.clear_if_unchanged(&second_token).await.unwrap();
    snapshot.expect_exact("first instance final");
    first.clear_if_unchanged(&first_token).await.unwrap();
    snapshot.expect_empty();
    drop(snapshot);
    drop(first);
    drop(second);
}
