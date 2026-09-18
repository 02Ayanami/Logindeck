use async_trait::async_trait;
use autologin_core::{AppError, Clipboard};
use secrecy::SecretString;

/// Opaque proof of the clipboard sequence created by one successful LoginDeck write.
#[derive(Eq, PartialEq)]
pub struct WindowsClipboardChangeToken(u32);

impl std::fmt::Debug for WindowsClipboardChangeToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WindowsClipboardChangeToken(<opaque>)")
    }
}

/// Bounded Windows clipboard implementation for time-limited password copies.
///
/// This boundary intentionally exposes no read operation. A change token can only clear the
/// exact clipboard sequence produced by its write; later clipboard contents are left untouched.
pub struct WindowsClipboard {
    #[cfg(target_os = "windows")]
    worker: native::WorkerSender,
}

impl WindowsClipboard {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "windows")]
            worker: native::start_worker(),
        }
    }
}

impl Default for WindowsClipboard {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Clipboard for WindowsClipboard {
    type ChangeToken = WindowsClipboardChangeToken;

    async fn write(&self, value: SecretString) -> Result<Self::ChangeToken, AppError> {
        #[cfg(target_os = "windows")]
        {
            use secrecy::ExposeSecret;

            if value.expose_secret().contains('\0') {
                return Err(AppError::invalid_field("password"));
            }
            let (reply, result) = tokio::sync::oneshot::channel();
            self.worker
                .dispatch(native::Command::Write { value, reply })
                .map_err(|_| unavailable())?;
            return result
                .await
                .map_err(|_| unavailable())?
                .map(WindowsClipboardChangeToken);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = value;
            Err(AppError::new("platform.unsupported"))
        }
    }

    async fn clear_if_unchanged(&self, token: &Self::ChangeToken) -> Result<(), AppError> {
        #[cfg(target_os = "windows")]
        {
            let expected = token.0;
            let (reply, result) = tokio::sync::oneshot::channel();
            self.worker
                .dispatch(native::Command::Clear { expected, reply })
                .map_err(|_| unavailable())?;
            return result.await.map_err(|_| unavailable())?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = token;
            Err(AppError::new("platform.unsupported"))
        }
    }
}

#[cfg(target_os = "windows")]
fn unavailable() -> AppError {
    AppError::new("clipboard.unavailable")
}

#[cfg(target_os = "windows")]
mod native {
    use std::{ptr, sync::Arc, sync::Mutex, thread, time::Duration};

    use autologin_core::AppError;
    use secrecy::{ExposeSecret, SecretString};
    use windows::{
        core::w,
        Win32::{
            Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND, WAIT_FAILED},
            System::{
                DataExchange::{
                    CloseClipboard, EmptyClipboard, GetClipboardOwner, GetClipboardSequenceNumber,
                    OpenClipboard, SetClipboardData,
                },
                Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
                Ole::CF_UNICODETEXT,
                Threading::{CreateEventW, SetEvent, INFINITE},
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, MsgWaitForMultipleObjectsEx,
                PeekMessageW, TranslateMessage, HWND_MESSAGE, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE,
                QS_ALLINPUT, WINDOW_EX_STYLE, WINDOW_STYLE,
            },
        },
    };
    use zeroize::Zeroizing;

    use super::unavailable;

    const OPEN_ATTEMPTS: usize = 5;
    const OPEN_RETRY_DELAY: Duration = Duration::from_millis(5);
    const CF_UNICODETEXT_ID: u32 = CF_UNICODETEXT.0 as u32;

    // Serialize LoginDeck's write and compare-and-clear operations. The guard is acquired only on
    // a blocking worker and never crosses an await.
    static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

    pub(super) enum Command {
        Write {
            value: SecretString,
            reply: tokio::sync::oneshot::Sender<Result<u32, AppError>>,
        },
        Clear {
            expected: u32,
            reply: tokio::sync::oneshot::Sender<Result<(), AppError>>,
        },
    }

    // Kernel handles are process-wide values. Store the raw value rather than windows::HANDLE,
    // whose pointer-shaped representation is intentionally not Send/Sync in the bindings.
    struct WakeEvent(isize);

    impl WakeEvent {
        fn handle(&self) -> HANDLE {
            HANDLE(self.0 as *mut std::ffi::c_void)
        }
    }

    impl Drop for WakeEvent {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.handle()) };
        }
    }

    pub(super) struct WorkerSender {
        sender: Option<std::sync::mpsc::Sender<Command>>,
        wake: Option<Arc<WakeEvent>>,
    }

    impl WorkerSender {
        pub(super) fn dispatch(&self, command: Command) -> Result<(), ()> {
            self.sender
                .as_ref()
                .ok_or(())?
                .send(command)
                .map_err(|_| ())?;
            let wake = self.wake.as_ref().ok_or(())?;
            unsafe { SetEvent(wake.handle()) }.map_err(|_| ())
        }
    }

    impl Drop for WorkerSender {
        fn drop(&mut self) {
            // Disconnect before signaling so the worker observes shutdown after draining commands.
            self.sender.take();
            if let Some(wake) = self.wake.as_ref() {
                let _ = unsafe { SetEvent(wake.handle()) };
            }
        }
    }

    pub(super) fn start_worker() -> WorkerSender {
        let (sender, receiver) = std::sync::mpsc::channel();
        let wake = unsafe { CreateEventW(None, false, false, None) }
            .ok()
            .map(|handle| WakeEvent(handle.0 as isize))
            .map(Arc::new);
        let Some(worker_wake) = wake.as_ref().map(Arc::clone) else {
            drop(receiver);
            return WorkerSender {
                sender: Some(sender),
                wake: None,
            };
        };
        // The dedicated thread owns the HWND for its complete lifetime and performs every
        // synchronous clipboard call. A spawn failure drops the receiver, so operations return
        // the same bounded unavailable error through the disconnected channel.
        let _ = thread::Builder::new()
            .name("logindeck-clipboard".to_owned())
            .spawn(move || {
                let owner = OwnerWindow::create();
                loop {
                    let mut disconnected = false;
                    loop {
                        match receiver.try_recv() {
                            Ok(command) => match command {
                                Command::Write { value, reply } => {
                                    let result = owner
                                        .as_ref()
                                        .map_err(|_| unavailable())
                                        .and_then(|owner| write(owner.0, value));
                                    if let Err(Ok(sequence)) = reply.send(result) {
                                        // If the async caller was cancelled after dispatch, do not
                                        // leave an untracked secret behind. The same sequence guard
                                        // preserves a replacement that raced with this cleanup.
                                        let _ =
                                            clear_if_unchanged(owner.as_ref().unwrap().0, sequence);
                                    }
                                }
                                Command::Clear { expected, reply } => {
                                    let result = owner
                                        .as_ref()
                                        .map_err(|_| unavailable())
                                        .and_then(|owner| clear_if_unchanged(owner.0, expected));
                                    let _ = reply.send(result);
                                }
                            },
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                disconnected = true;
                                break;
                            }
                        }
                    }

                    // A real owner window must dispatch WM_DESTROYCLIPBOARD and related messages;
                    // otherwise another application can stall waiting for this process.
                    let mut message = MSG::default();
                    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                        unsafe {
                            let _ = TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                    if disconnected {
                        break;
                    }

                    let wait = unsafe {
                        MsgWaitForMultipleObjectsEx(
                            Some(&[worker_wake.handle()]),
                            INFINITE,
                            QS_ALLINPUT,
                            MWMO_INPUTAVAILABLE,
                        )
                    };
                    if wait == WAIT_FAILED {
                        break;
                    }
                }
            });
        WorkerSender {
            sender: Some(sender),
            wake,
        }
    }

    struct OwnerWindow(HWND);

    impl OwnerWindow {
        fn create() -> Result<Self, AppError> {
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
            }
            .map_err(|_| unavailable())?;
            Ok(Self(window))
        }
    }

    impl Drop for OwnerWindow {
        fn drop(&mut self) {
            // SAFETY: this thread created the window and the guard destroys it exactly once.
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    struct OpenClipboardGuard {
        open: bool,
    }

    impl OpenClipboardGuard {
        fn open(owner: HWND) -> Result<Self, AppError> {
            for attempt in 0..OPEN_ATTEMPTS {
                // SAFETY: owner is a live window created on this blocking worker thread.
                if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
                    return Ok(Self { open: true });
                }
                if attempt + 1 < OPEN_ATTEMPTS {
                    thread::sleep(OPEN_RETRY_DELAY);
                }
            }
            Err(unavailable())
        }

        fn close(mut self) -> Result<(), AppError> {
            if unsafe { CloseClipboard() }.is_err() {
                // Leave the guard armed so Drop makes one final best-effort close attempt.
                return Err(unavailable());
            }
            self.open = false;
            Ok(())
        }
    }

    impl Drop for OpenClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: this guard exists only after OpenClipboard succeeded on the same thread.
            if self.open {
                let _ = unsafe { CloseClipboard() };
            }
        }
    }

    /// Movable global memory owned by LoginDeck until `transfer` follows SetClipboardData.
    struct OwnedGlobal {
        handle: Option<HGLOBAL>,
        secret_bytes: usize,
    }

    impl OwnedGlobal {
        fn allocate(byte_len: usize) -> Result<Self, AppError> {
            // SAFETY: the returned handle is held by this RAII guard until transfer or drop.
            let handle =
                unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }.map_err(|_| unavailable())?;
            Ok(Self {
                handle: Some(handle),
                secret_bytes: 0,
            })
        }

        fn handle(&self) -> HGLOBAL {
            self.handle.expect("global memory guard must be armed")
        }

        fn copy_from(&mut self, source: &[u16]) -> Result<(), AppError> {
            let byte_len = source
                .len()
                .checked_mul(size_of::<u16>())
                .ok_or_else(unavailable)?;
            let handle = self.handle();
            // SAFETY: handle is a live GMEM_MOVEABLE allocation of exactly byte_len bytes.
            let destination = unsafe { GlobalLock(handle) };
            if destination.is_null() {
                return Err(unavailable());
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    source.as_ptr().cast::<u8>(),
                    destination.cast::<u8>(),
                    byte_len,
                );
                // Releasing the final lock reports zero even on success, so the Result is ignored.
                let _ = GlobalUnlock(handle);
            }
            self.secret_bytes = byte_len;
            Ok(())
        }

        fn clipboard_handle(&self) -> HANDLE {
            HANDLE(self.handle().0)
        }

        fn transfer(mut self) {
            self.handle = None;
            self.secret_bytes = 0;
        }
    }

    impl Drop for OwnedGlobal {
        fn drop(&mut self) {
            let Some(handle) = self.handle.take() else {
                return;
            };
            if self.secret_bytes > 0 {
                // Best effort: erase a failed write before releasing memory still owned here.
                let destination = unsafe { GlobalLock(handle) };
                if !destination.is_null() {
                    unsafe {
                        ptr::write_bytes(destination.cast::<u8>(), 0, self.secret_bytes);
                        let _ = GlobalUnlock(handle);
                    }
                }
            }
            // SAFETY: SetClipboardData did not take this armed allocation; free it exactly once.
            let _ = unsafe { GlobalFree(Some(handle)) };
        }
    }

    fn write(owner: HWND, value: SecretString) -> Result<u32, AppError> {
        let _serial = CLIPBOARD_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let wide = Zeroizing::new(
            value
                .expose_secret()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>(),
        );
        let byte_len = wide
            .len()
            .checked_mul(size_of::<u16>())
            .ok_or_else(unavailable)?;

        let _open = OpenClipboardGuard::open(owner)?;
        // SAFETY: the clipboard is exclusively open with a valid owner window.
        unsafe { EmptyClipboard() }.map_err(|_| unavailable())?;

        let mut memory = OwnedGlobal::allocate(byte_len)?;
        memory.copy_from(&wide)?;
        // SAFETY: the movable allocation is unlocked and the clipboard remains exclusively open.
        unsafe { SetClipboardData(CF_UNICODETEXT_ID, Some(memory.clipboard_handle())) }
            .map_err(|_| unavailable())?;
        // System ownership starts only after SetClipboardData succeeds.
        memory.transfer();
        // Windows finalizes the clipboard sequence as CloseClipboard publishes the update.
        _open.close()?;
        let sequence = unsafe { GetClipboardSequenceNumber() };
        // Sampling occurs after close, so verify that no external writer won that small handoff.
        // The owner HWND is metadata only; production never reads clipboard contents.
        let observed_owner = unsafe { GetClipboardOwner() }.map_err(|_| unavailable())?;
        if observed_owner != owner {
            return Err(unavailable());
        }
        Ok(sequence)
    }

    fn clear_if_unchanged(owner: HWND, expected: u32) -> Result<(), AppError> {
        let _serial = CLIPBOARD_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // First avoid opening or mutating the clipboard when a replacement is already visible.
        if unsafe { GetClipboardSequenceNumber() } != expected {
            return Ok(());
        }

        let _open = OpenClipboardGuard::open(owner)?;
        // Opening excludes other writers. Re-check to close the race between the first observation
        // and exclusive ownership; any intervening replacement is preserved.
        if unsafe { GetClipboardSequenceNumber() } != expected {
            return Ok(());
        }
        unsafe { EmptyClipboard() }.map_err(|_| unavailable())
    }
}
